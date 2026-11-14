//! What can be done to a message from its menu: copying its text or a link
//! to it, pinning, and reactions. (Editing and deleting follow.)
//!
//! Each follows desktop's handling in `src/ui/chat.rs`.

use stoat_gpui::emoji::{self, EmojiChoice};
use stoat_gpui::links;
use stoat_gpui::runtime::spawn_io;
use stoat_gpui::store::{FULL_REACTIONS_TEXT, TimelineRow};

use crate::CoreError;
use crate::controller::AccountController;
use crate::timeline::{EmojiItem, PickerEmojiItem};

/// What a message's reaction picker offers.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReactionPicker {
    pub choices: Vec<PickerEmojiItem>,
    /// The message has as many different emoji as it can: only its own are
    /// offered (joining one still works).
    pub full: bool,
    /// Its author allowed only some emoji: only those are offered.
    pub restricted: bool,
    /// Why the choices are limited, to show above them.
    pub note: Option<String>,
}

/// Most results the picker's search returns.
const SEARCH_LIMIT: usize = 200;

#[uniffi::export]
impl AccountController {
    /// Message `message_id`'s text as typed (its Markdown source), for
    /// "Copy Text".
    pub fn raw_text(&self, channel_id: String, message_id: String) -> Option<String> {
        let store = self.shared.store.lock().unwrap();
        store
            .raw_content(&channel_id, &message_id)
            .map(str::to_string)
    }

    /// A link to the message on the instance's web client, for "Copy
    /// Link". `None` until `GET /` has answered, or if the instance has no
    /// web client.
    pub fn message_link(&self, channel_id: String, message_id: String) -> Option<String> {
        let app = self.shared.web_app.lock().unwrap().clone()?;
        let store = self.shared.store.lock().unwrap();
        let server = store.channel_server(&channel_id);
        Some(links::message_link(&app, server, &channel_id, &message_id))
    }

    /// Whether the user may pin and unpin messages in `channel_id`.
    pub fn can_pin(&self, channel_id: String) -> bool {
        self.shared.store.lock().unwrap().can_pin(&channel_id)
    }

    /// Pin (`pinned`) or unpin a message. As on desktop, the row changes
    /// when Stoat's update says so; if someone got there first, it's
    /// caught up here. A refusal returns desktop's text.
    pub async fn set_pinned(
        &self,
        channel_id: String,
        message_id: String,
        pinned: bool,
    ) -> Result<(), CoreError> {
        let client = self.shared.client.clone();
        let request = spawn_io({
            let (channel, id) = (channel_id.clone(), message_id.clone());
            async move {
                if pinned {
                    client.pin_message(&channel, &id).await
                } else {
                    client.unpin_message(&channel, &id).await
                }
            }
        });
        match request.await.map_err(CoreError::failed)? {
            Ok(()) => Ok(()),
            // Already as asked: the update that said so was missed.
            Err(err) if err.pin_already_done() => {
                let changed =
                    self.shared
                        .store
                        .lock()
                        .unwrap()
                        .set_pinned(&channel_id, &message_id, pinned);
                if changed {
                    self.shared.timeline_changed(&channel_id);
                }
                Ok(())
            }
            Err(err) => {
                log::warn!("couldn't pin={pinned} {message_id} in {channel_id}: {err:?}");
                Err(CoreError::failed(err.pin_failure_text(pinned)))
            }
        }
    }
}

/// Reactions.
#[uniffi::export]
impl AccountController {
    /// What the reaction picker offers for message `message_id`: its
    /// existing emoji when it's full, the emoji its author allowed, or the
    /// channel's usual choices (common Unicode emoji, then custom ones).
    pub fn reaction_picker(&self, channel_id: String, message_id: String) -> ReactionPicker {
        let store = self.shared.store.lock().unwrap();
        let usual = store.reaction_choices(&channel_id);
        let row = store.timeline(&channel_id).into_iter().find_map(|row| match row {
            TimelineRow::Message(row) if row.id == message_id => Some(row),
            _ => None,
        });
        let Some(row) = row else {
            return ReactionPicker {
                choices: usual.iter().map(PickerEmojiItem::of).collect(),
                full: false,
                restricted: false,
                note: None,
            };
        };
        let restricted = row.allowed_reactions.is_some();
        let picked = row.picker_choices(&usual);
        ReactionPicker {
            choices: picked.emoji.iter().map(PickerEmojiItem::of).collect(),
            full: picked.full,
            restricted,
            note: if picked.full {
                Some(FULL_REACTIONS_TEXT.into())
            } else if restricted {
                Some("Its author allowed only these reactions.".into())
            } else {
                None
            },
        }
    }

    /// Emoji matching `query` (a shortcode or name, as after `:`), custom
    /// ones usable in `channel_id` first, best first.
    pub fn search_emoji(&self, channel_id: String, query: String) -> Vec<PickerEmojiItem> {
        let custom = self.shared.store.lock().unwrap().custom_emoji_for(&channel_id);
        emoji::search_all(query.trim().trim_matches(':'), &custom, SEARCH_LIMIT)
            .into_iter()
            .map(|choice| match choice {
                EmojiChoice::Unicode { emoji, .. } => PickerEmojiItem {
                    key: emoji.to_string(),
                    emoji: EmojiItem::Unicode {
                        text: emoji.to_string(),
                    },
                },
                EmojiChoice::Custom(custom) => PickerEmojiItem {
                    key: custom.id.clone(),
                    emoji: EmojiItem::Custom {
                        id: custom.id,
                        name: format!(":{}:", custom.name),
                        url: custom.url,
                    },
                },
            })
            .collect()
    }

    /// Add the user's reaction `emoji` (as the API names it) to a message,
    /// or take it back if it's there. It shows at once; if Stoat refuses,
    /// it's undone and desktop's text returned. A new emoji on a full
    /// message is refused without asking Stoat.
    pub async fn toggle_reaction(
        &self,
        channel_id: String,
        message_id: String,
        emoji: String,
    ) -> Result<(), CoreError> {
        let pending = (message_id.clone(), emoji.clone());
        if !self.shared.reacting.lock().unwrap().insert(pending.clone()) {
            // The last tap on it is still on its way.
            return Ok(());
        }
        let shared = self.shared.clone();
        let done = move || {
            shared.reacting.lock().unwrap().remove(&pending);
        };
        let (add, me) = {
            let mut store = self.shared.store.lock().unwrap();
            let add = !store.has_reacted(&channel_id, &message_id, &emoji);
            if add && store.would_exceed_reactions(&channel_id, &message_id, &emoji) {
                drop(store);
                done();
                return Err(CoreError::failed(FULL_REACTIONS_TEXT));
            }
            let me = store.me().to_string();
            store.apply_react(&channel_id, &message_id, &me, &emoji, add);
            (add, me)
        };
        self.shared.timeline_changed(&channel_id);

        let client = self.shared.client.clone();
        let request = spawn_io({
            let (channel, message, emoji) = (channel_id.clone(), message_id.clone(), emoji.clone());
            async move {
                if add {
                    client.react(&channel, &message, &emoji).await
                } else {
                    client.unreact(&channel, &message, &emoji).await
                }
            }
        });
        let result = request.await;
        done();
        let error = match result {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(err)) => {
                log::warn!("couldn't change reaction {emoji} on {message_id}: {err:?}");
                err.reaction_failure_text()
            }
            Err(err) => {
                log::warn!("reaction task failed: {err:#}");
                "Couldn't update your reaction.".into()
            }
        };
        let undone = self
            .shared
            .store
            .lock()
            .unwrap()
            .apply_react(&channel_id, &message_id, &me, &emoji, !add);
        if undone {
            self.shared.timeline_changed(&channel_id);
        }
        Err(CoreError::failed(error))
    }
}
