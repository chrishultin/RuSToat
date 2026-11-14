//! What can be done to a message from its menu: copying its text or a link
//! to it, and pinning. (Reactions, editing and deleting follow.)
//!
//! Each follows desktop's handling in `src/ui/chat.rs`.

use stoat_gpui::links;
use stoat_gpui::runtime::spawn_io;

use crate::CoreError;
use crate::controller::AccountController;

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
