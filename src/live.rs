//! Applying live events to a [`ChatStore`], without any UI.
//!
//! This is the store half of desktop's event dispatch
//! (`ChatView::on_connection_event` in `src/ui/chat.rs`), for clients that
//! keep no view state of their own beside the store (the iPhone app). What
//! desktop does around each call (panes, notices, member-list fetches) is
//! left to the caller. When one changes, check the other.

use crate::api::events::ServerEvent;
use crate::store::ChatStore;

/// What an event changed, so the caller knows what to read again.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Changed {
    /// A fresh `Ready` replaced everything.
    pub ready: bool,
    /// The server, channel or conversation lists, or their unread state.
    pub lists: bool,
    /// Channels whose loaded messages changed.
    pub timelines: Vec<String>,
}

impl Changed {
    fn lists() -> Self {
        Self {
            lists: true,
            ..Self::default()
        }
    }

    fn timeline_if(changed: bool, channel: String) -> Self {
        Self {
            timelines: if changed { vec![channel] } else { Vec::new() },
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.ready && !self.lists && self.timelines.is_empty()
    }

    /// Fold `other` into this one (for coalescing a burst of events).
    pub fn merge(&mut self, other: Changed) {
        self.ready |= other.ready;
        self.lists |= other.lists;
        for channel in other.timelines {
            if !self.timelines.contains(&channel) {
                self.timelines.push(channel);
            }
        }
    }
}

/// Apply one event's effect on `store`. `now_ms` is the current time, for
/// slowmode cooldowns.
pub fn apply_event(store: &mut ChatStore, event: ServerEvent, now_ms: u64) -> Changed {
    match event {
        ServerEvent::Ready(ready) => {
            store.apply_ready(*ready);
            Changed {
                ready: true,
                lists: true,
                timelines: Vec::new(),
            }
        }
        ServerEvent::Message(message) => {
            let channel = message.channel.clone();
            let shown = store.apply_message(*message);
            // A new message changes its channel's unread state and Home's order.
            Changed {
                lists: true,
                ..Changed::timeline_if(shown, channel)
            }
        }
        ServerEvent::MessageUpdate {
            id,
            channel,
            data,
            clear,
        } => {
            let shown = store.apply_update(&channel, &id, *data, &clear);
            Changed::timeline_if(shown, channel)
        }
        ServerEvent::MessageDelete { id, channel } => {
            let shown = store.apply_delete(&channel, &[id]);
            Changed::timeline_if(shown, channel)
        }
        ServerEvent::BulkMessageDelete { channel, ids } => {
            let shown = store.apply_delete(&channel, &ids);
            Changed::timeline_if(shown, channel)
        }
        ServerEvent::MessageAppend {
            id,
            channel,
            embeds,
        } => {
            let shown = store.apply_append(&channel, &id, embeds);
            Changed::timeline_if(shown, channel)
        }
        ServerEvent::MessageReact {
            id,
            channel,
            user,
            emoji,
        } => {
            let shown = store.apply_react(&channel, &id, &user, &emoji, true);
            Changed::timeline_if(shown, channel)
        }
        ServerEvent::MessageUnreact {
            id,
            channel,
            user,
            emoji,
        } => {
            let shown = store.apply_react(&channel, &id, &user, &emoji, false);
            Changed::timeline_if(shown, channel)
        }
        ServerEvent::MessageRemoveReaction { id, channel, emoji } => {
            let shown = store.apply_remove_reaction(&channel, &id, &emoji);
            Changed::timeline_if(shown, channel)
        }
        ServerEvent::ChannelAck {
            channel,
            user,
            message_id,
        } => {
            if user != store.me() {
                return Changed::default();
            }
            store.apply_ack(&channel, &message_id);
            // The unread divider moves too.
            Changed {
                lists: true,
                ..Changed::timeline_if(true, channel)
            }
        }
        ServerEvent::ChannelCreate(channel) => {
            store.add_channel(*channel);
            Changed::lists()
        }
        ServerEvent::ChannelUpdate { id, data, clear } => {
            store.apply_channel_update(&id, *data, &clear);
            Changed {
                lists: true,
                ..Changed::timeline_if(true, id)
            }
        }
        ServerEvent::ChannelDelete { id } => {
            if store.remove_channel(&id) {
                Changed::lists()
            } else {
                Changed::default()
            }
        }
        ServerEvent::ChannelGroupJoin { channel, user } => {
            if store.add_recipient(&channel, &user) {
                Changed::lists()
            } else {
                Changed::default()
            }
        }
        ServerEvent::ChannelGroupLeave { channel, user } => {
            // Removed, or left on another device: Stoat sends no
            // `ChannelDelete` for it.
            let changed = if user == store.me() {
                store.remove_channel(&channel)
            } else {
                store.remove_recipient(&channel, &user)
            };
            if changed {
                Changed::lists()
            } else {
                Changed::default()
            }
        }
        ServerEvent::EmojiCreate(emoji) => {
            store.add_emoji(*emoji);
            Changed::default()
        }
        ServerEvent::EmojiDelete { id } => {
            store.remove_emoji(&id);
            Changed::default()
        }
        ServerEvent::ServerMemberJoin { member, .. } => {
            store.apply_member_join(*member);
            Changed::lists()
        }
        ServerEvent::ServerMemberLeave { server, user } => {
            // True when it's the user who left (or was kicked or banned).
            if store.apply_member_leave(&server, &user) {
                store.remove_server(&server);
            }
            Changed::lists()
        }
        ServerEvent::ServerMemberUpdate { id, data, clear } => {
            // Roles decide which channels show.
            store.apply_member_update(&id, *data, &clear);
            Changed::lists()
        }
        ServerEvent::UserUpdate { id, data, clear } => {
            // DM names and avatars.
            store.apply_user_update(&id, *data, &clear);
            Changed::lists()
        }
        ServerEvent::ServerUpdate { id, data, clear } => {
            if store.apply_server_update(&id, *data, &clear) {
                Changed::lists()
            } else {
                Changed::default()
            }
        }
        ServerEvent::ServerRoleUpdate {
            server,
            role_id,
            data,
            clear,
        } => {
            if store.apply_role_update(&server, &role_id, *data, &clear) {
                Changed::lists()
            } else {
                Changed::default()
            }
        }
        ServerEvent::ServerRoleDelete { server, role_id } => {
            if store.apply_role_delete(&server, &role_id) {
                Changed::lists()
            } else {
                Changed::default()
            }
        }
        ServerEvent::ServerRoleRanksUpdate { server, ranks } => {
            if store.apply_role_ranks(&server, &ranks) {
                Changed::lists()
            } else {
                Changed::default()
            }
        }
        ServerEvent::ServerCreate {
            server,
            channels,
            emojis,
            voice_states,
        } => {
            let id = server.id.clone();
            store.add_server(*server, channels, emojis);
            store.set_server_voice(&id, voice_states);
            Changed::lists()
        }
        ServerEvent::ServerDelete { id } => {
            store.remove_server(&id);
            Changed::lists()
        }
        ServerEvent::UserRelationship { user } => {
            store.apply_relationship(*user);
            Changed::lists()
        }
        ServerEvent::UserSettingsUpdate {
            notifications,
            ordering,
            server_folders,
        } => {
            let mut changed = false;
            if let Some((updated, settings)) = notifications {
                changed |= store.set_notification_settings(updated, *settings);
            }
            if let Some((updated, ordering)) = ordering {
                changed |= store.set_ordering(updated, *ordering);
            }
            if let Some((updated, folders)) = server_folders {
                changed |= store.set_server_folders(updated, *folders);
            }
            if changed {
                Changed::lists()
            } else {
                Changed::default()
            }
        }
        ServerEvent::UserSlowmodes(slowmodes) => {
            store.apply_slowmodes(&slowmodes, now_ms);
            Changed::default()
        }
        ServerEvent::VoiceChannelJoin { channel, state } => {
            store.apply_voice_join(&channel, *state);
            Changed::default()
        }
        ServerEvent::VoiceChannelLeave { channel, user } => {
            store.apply_voice_leave(&channel, &user);
            Changed::default()
        }
        ServerEvent::VoiceChannelMove {
            from, to, state, ..
        } => {
            store.apply_voice_move(&from, &to, *state);
            Changed::default()
        }
        ServerEvent::UserVoiceStateUpdate {
            user,
            channel,
            data,
        } => {
            store.apply_voice_update(&channel, &user, *data);
            Changed::default()
        }
        ServerEvent::Authenticated
        | ServerEvent::Pong
        | ServerEvent::Error(_)
        | ServerEvent::Logout
        | ServerEvent::ChannelStartTyping { .. }
        | ServerEvent::ChannelStopTyping { .. }
        | ServerEvent::UserMoveVoiceChannel(_)
        | ServerEvent::Unknown(_) => Changed::default(),
    }
}
