# Roadmap: closing the gap with Stoat's web client

## Summary
This roadmap lists what Stoat's web client can do that RuSToat can't yet, and splits it into milestones. Each milestone is **no larger than M**: one plan of 4–8 chunks, like milestones 2–8. Milestones 1–8 are done. This roadmap starts at **milestone 9**.

Sizes: **S** is a chunk or two. **M** is a whole milestone. Anything larger has been split.

Each milestone gets its own detailed plan (`docs/plans/<name>.md`) when it's next up, not before. Plans written now would rest on code that will have changed by the time they're used. Only milestone 9 is planned in detail so far: [`reading-flow.md`](reading-flow.md).

## Where the list came from
On 2026-11-06 we compared RuSToat with three sources:
- Stoat's web client, `stoatchat/for-web` v0.16.3
- the older `revoltchat/revite`
- the live API (`api.stoat.chat`, version 0.16.2, and its OpenAPI spec)

RuSToat already has:
- login and 2FA, and several accounts at once
- servers, channels, DMs and groups
- rich messages, reactions, replies, editing, deleting and pinning
- attachments, and autocomplete in the message box
- the member list, search in a channel, and pins
- unreads, the Dock badge and macOS banners
- profile cards and the member menu
- Friends

## Decisions
1. **Order:** daily polish first, then your own settings, then joining and organising, then moderation and server management, then bots, then media, then voice.
2. **Voice and video start with a spike.** It proves the LiveKit Rust SDK builds and plays audio next to GPUI. Milestones 33–37 happen only if the spike says go.
3. **Out of scope:** language selection (i18n), sign-up and onboarding (including email verification and password reset), the Discover page, disabling or deleting your account, the server audit log (the web client doesn't use it either), and revite-only extras (custom CSS, transparency, seasonal themes).
4. **In scope, despite earlier doubts:** managing bots (milestone 29) and basic theming, meaning a choice from a fixed set of colour palettes (milestone 13).
5. **Server-wide search is blocked.** API 0.16.2 has no endpoint for it.

## Shared groundwork
Some milestones lay foundations that later ones reuse. When the earlier one's plan is written, check what the later ones need from it.

| Built in | Groundwork | Reused by |
|---|---|---|
| 13 | A settings window with sections | 14–17, 23–28, 29 |
| 14 | Writing `/sync/settings` (notification settings, ordering), not just reading it | 21 |
| 18 | Invite cards and the join flow | 29 (adding a bot to a server) |
| 25 | The permission matrix | 27 (channel overrides) |
| 32 | The go/no-go decision on LiveKit | 33–37 |

## A. Daily polish

### Milestone 9: Reading flow (M). Plan: [`reading-flow.md`](reading-flow.md)
- A "New messages" divider where you left off
- Mark as unread
- Copy message link or ID; copy channel, server or category ID
- Collapsible channel categories, remembered per account
- Keyboard shortcuts: move between channels and servers, jump to present, mark a channel or server read, switch accounts

### Milestone 10: Message markup (M). Plan: [`message-markup.md`](message-markup.md)
- Role mentions (`<%role>`), `@everyone` and `@online`: drawn in the role's colour, offered in autocomplete, and counted as mentions when they include you
- `<t:…>` timestamps, shown in your time zone, or as relative time ("in 3 hours") where the tag asks for it
- Text spoilers that stay hidden until clicked
- A copy button on code blocks
- Quote a message into the message box (from the message menu)

### Milestone 11: Safety (M, small)
- Blocked users' messages collapsed to "N blocked messages", which you can click to show
- A warning before opening a link whose text and address differ
- An age gate on mature (NSFW) channels

### Milestone 12: Search, slowmode and GIFs (M)
- Search sort (relevance, latest or oldest), and "Load more" past 50 results
- Slowmode: show the channel's slowmode, and count down in the message box after you send
- A GIF picker (the web client's GIF search)

## B. You

### Milestone 13: Settings window and themes (M). Plan: [`settings-and-themes.md`](settings-and-themes.md)
- A settings window (⌘,) with sections in a sidebar. Later milestones add their own sections.
- **Themes:** pick from a fixed set of colour palettes, each with a light and a dark variant, or follow the system. Built on gpui-component's theme registry.
- **App behaviour:** start at login, and keep running in the background when the window closes, so notifications continue. Today closing the window disconnects every account.

### Milestone 14: Status and notifications (M)
- Set your presence (Online, Idle, Focus, Busy, Invisible) and a custom status, from your avatar
- Edit notification levels (All, Mentions, None) and muting, for a set time or until you unmute, per server and per channel, from their right-click menus. This writes Stoat's synced settings, so it carries across clients.
- Notification sounds, with an on/off switch

### Milestone 15: Your profile (M)
- Edit your display name, avatar, banner, bio and pronouns
- A per-server identity for yourself (nickname and avatar)
- No crop editor: images are centre-cropped. Add one later if it's missed.

### Milestone 16: Security (M)
- Sessions: list, rename, revoke one, or log out every other session
- Two-factor: turn TOTP on or off (with a QR code), and view or regenerate recovery codes

### Milestone 17: Account details (S)
- Change your username, email and password, each confirmed with your current password

## C. Joining and organising

### Milestone 18: Invites (M)
- Join a server from an invite code or link, from a "Join server" dialog
- Invite links in messages show as cards with a Join button
- Create an invite for a channel and copy its link

### Milestone 19: Create, leave and delete servers and channels (M)
- Create a server
- Create a channel or category (with Manage Channels)
- Leave a server
- Delete a channel or server (with confirmation, for owners and admins)
- Server and channel info dialogs (descriptions), from the header

### Milestone 20: Groups and DMs (M)
- Create a group, including from the Friends page
- Add or remove group members
- Leave a group, or transfer its ownership
- Close a DM
- Mutual friends, servers and groups on profile cards

### Milestone 21: Server list and accounts (M)
- Server folders
- Drag to reorder servers (synced through `/sync/settings`, as the web client does)
- Drag to reorder accounts in the switcher, and a shortcut for each (⌘1–9, if milestone 9 hasn't already added them)

## D. Moderation and server management

### Milestone 22: Member moderation (M)
- From the member menu: kick; ban (with a reason, and optionally deleting recent messages); time out or end a timeout
- Edit a member's nickname or avatar, and their roles
- Report a message, user or server (`POST /safety/report`)

### Milestone 23: Server settings: overview, invites, bans (M)
- A server settings window, reusing milestone 13's window
- Overview: name, description, icon, banner, and the channels for system messages
- Invites: list and revoke
- Bans: list and unban
- Transfer server ownership

### Milestone 24: Roles editor, part 1 (M)
- List roles, and create, delete or rename one
- Colour, hoisting and icon
- Reorder roles by rank
- This needs `ServerRoleRanksUpdate` events, as live role edits (milestone 7) already handle

### Milestone 25: Roles editor, part 2: permissions (M)
- A permission matrix for a role, and for the server's default permissions, built from `revolt-permissions`' list of permissions
- Greys out what you can't grant

### Milestone 26: Channel settings and order (M)
- Channel overview: name, description, icon, slowmode, mature flag
- Reorder channels, and move them between categories; edit categories

### Milestone 27: Channel permission overrides (M)
- Allow or deny, per role and for the default permissions, reusing milestone 25's matrix

### Milestone 28: Emoji and webhooks (M)
- Server emoji: upload, rename, delete
- Webhooks: create, view (copy the URL), delete

## E. Bots

### Milestone 29: Bots (M)
- Your bots: list, create, edit (name, avatar, public or private, interactions URL), reset and copy the token, delete
- Add a bot to a server or group, from its invite
- Profile cards show bot badges and the bot's owner

## F. Media

### Milestone 30: Attachments and images (M)
- Image and file actions: Save, Copy image, Copy link, Open in browser, also in the image viewer's right-click menu
- Mark an upload as a spoiler (new in 0.16); spoiler attachments stay blurred until clicked
- Inline previews of text-file attachments

### Milestone 31: Inline audio and video (M)
- An audio player in the message, through AVFoundation (`AVPlayer` via `objc2`), with play/pause and seeking drawn in GPUI
- Videos show a poster frame with a play button that opens a native player window. Playing video inside the message would be L, and is left out unless the player window turns out not to be enough.
- **Risk:** the first use of AVFoundation here. Start with a small spike.

## G. Voice and video (only if milestone 32 says go)

### Milestone 32: Voice spike (S–M): a go/no-go gate
- Add the LiveKit Rust SDK (`livekit` crate), built next to GPUI on macOS
- Join one voice channel, audio only, with the default devices. The root config lists LiveKit nodes under `features.livekit`.
- Write up the result: build time, binary size, what the crate needs (WebRTC), whether audio works, and any rough edges. Decide go or no-go.

### Milestone 33: Voice channels (M)
- Join and leave a voice channel
- Who's in each channel, in the channel list, with speaking indicators
- Mute and deafen

### Milestone 34: Devices and audio processing (M)
- Choose input and output devices
- Per-person volume and mute
- Noise suppression and echo cancellation (WebRTC's audio processing)

### Milestone 35: DM and group calls (M)
- Start a call
- An incoming call rings, with a banner when RuSToat is in the background
- Call cards in the chat

### Milestone 36: Camera video (M)
- Send and receive camera video, with frames drawn into GPUI

### Milestone 37: Screen sharing (M)
- Share a screen or window (ScreenCaptureKit), and watch someone else's share

## Smaller leftovers (fit into whichever milestone is nearby)
- Notices from milestone 7's record: system-message names have no right-click menu, and `cargo fmt --check` fails on formatting drift that predates milestone 7
- Clicking an old banner while the window is closed doesn't reopen it (from the Dock-reopen change)
- A choice of Unicode emoji style (the web client offers several emoji packs)
