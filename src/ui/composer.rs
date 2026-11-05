//! The message box under a conversation. Enter sends, Shift+Enter adds a
//! new line, and each channel keeps its own draft, replies and attached
//! files included. `@` suggests people, and mentions show as chips.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{
    Enter, Escape, IndentInline, InlineToken, InlineTokenContext, InputContent, InputEvent,
    MoveDown, MoveUp, Position, Textarea, TextareaState,
};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::api::models::ReplyIntent;
use crate::attachment::FileKind;
use crate::complete::{Insertion, Query, Suggestion, TokenRef, completion_query, mention_spans};
use crate::draft::{
    AttachmentLimits, Draft, DraftAttachment, Drafts, EditStash, MAX_MESSAGE_CHARS, ReplyTarget,
    SendCheck, TooManyReplies, char_count, check_send, save_pasted_image,
};

pub enum ComposerEvent {
    /// Send this text (already checked and trimmed, and empty only if
    /// there are files) in the composer's channel, replying to `replies`,
    /// with `attachments`.
    Send {
        text: String,
        replies: Vec<ReplyIntent>,
        attachments: Vec<DraftAttachment>,
    },
    /// Save new text for the user's message `id` (already checked and trimmed).
    Edit { id: String, content: String },
    /// Up in an empty box: edit the user's last message.
    EditLast,
    /// The user typed (or cleared the box): `empty` if nothing's left. Not
    /// sent while editing, which isn't typing a message.
    Input { empty: bool },
    /// Something's being completed: answer with [`Composer::set_suggestions`].
    Complete(Query),
}

/// What gives a chip its label (`@Ada`) when text is loaded into the box.
pub type Labeler<'a> = &'a dyn Fn(&TokenRef) -> String;

/// The suggestion list above the box.
struct Completion {
    query: Query,
    suggestions: Vec<Suggestion>,
    /// More may come (the server's members are loading).
    loading: bool,
    selected: usize,
}

/// Shown when an edit would leave the message empty (Stoat needs at least
/// one character).
const EMPTY_EDIT_TEXT: &str = "To delete the message, use Delete.";

pub struct Composer {
    input: Entity<TextareaState>,
    /// The channel being typed in.
    channel: Option<String>,
    /// What the message replies to, and its files. (Its text lives in `input`.)
    draft: Draft,
    /// How many files a message may have, and how big (from `GET /`).
    limits: AttachmentLimits,
    drafts: Drafts,
    /// Editing one of the user's messages, and the draft set aside meanwhile.
    editing: EditStash,
    /// Why Enter didn't do anything (cleared when the text changes).
    hint: Option<SharedString>,
    /// What's being completed, and the suggestions for it.
    completion: Option<Completion>,
    /// Chip labels seen so far, by token ID, for text that comes back
    /// (a draft after an edit).
    labels: HashMap<String, String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ComposerEvent> for Composer {}

/// Rows the message box grows to before it scrolls.
const MAX_ROWS: usize = 8;

impl Composer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, MAX_ROWS)
                // Enter emits `PressEnter` without inserting a newline;
                // Shift+Enter still inserts one.
                .submit_on_enter(true)
        });
        let subscriptions = vec![cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { shift: false, .. } => this.submit(window, cx),
                // The character count follows the text.
                InputEvent::Change => {
                    this.hint = None;
                    if this.editing.editing().is_none() {
                        let empty = this.input.read(cx).value().trim().is_empty();
                        cx.emit(ComposerEvent::Input { empty });
                    }
                    this.update_completion(window, cx);
                    cx.notify()
                }
                InputEvent::Blur => {
                    this.completion = None;
                    cx.notify()
                }
                _ => {}
            },
        )];
        Self {
            input,
            channel: None,
            draft: Draft::default(),
            limits: AttachmentLimits::default(),
            drafts: Drafts::default(),
            editing: EditStash::default(),
            hint: None,
            completion: None,
            labels: HashMap::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn channel(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    /// Type in `channel` from now on: the current text and replies are kept
    /// as the old channel's draft, and `channel`'s own draft comes back,
    /// its mentions labelled by `labels`.
    pub fn set_channel(
        &mut self,
        channel: Option<&str>,
        placeholder: SharedString,
        labels: Labeler,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.channel.as_deref() == channel {
            return;
        }
        // Leaving mid-edit cancels it; the draft underneath is kept.
        self.cancel_edit(window, cx);
        self.hint = None;
        if let Some(old) = self.channel.clone() {
            let draft = self.take_draft(cx);
            self.drafts.save(&old, draft);
        }
        let Draft {
            text,
            replies,
            attachments,
        } = channel.map(|c| self.drafts.take(c)).unwrap_or_default();
        self.draft.replies = replies;
        self.draft.attachments = attachments;
        self.channel = channel.map(str::to_string);
        self.input
            .update(cx, |input, cx| input.set_placeholder(placeholder, window, cx));
        self.learn_labels(&text, labels);
        self.set_text(&text, window, cx);
        cx.notify();
    }

    /// The whole draft (text, replies and files), leaving the replies and
    /// files empty. The text stays in the box.
    fn take_draft(&mut self, cx: &App) -> Draft {
        Draft {
            text: self.input.read(cx).value().to_string(),
            ..std::mem::take(&mut self.draft)
        }
    }

    /// How many files a message may have, and how big.
    pub fn set_limits(&mut self, limits: AttachmentLimits) {
        self.limits = limits;
    }

    /// Ask for files to attach (the paperclip).
    fn pick_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| this.add_files(paths, window, cx))
                .ok();
        })
        .detach();
    }

    /// Attach the files at `paths` (picked or dropped; folders are
    /// skipped). Any that are too big, or past the limit, are refused with
    /// a toast.
    pub fn add_files(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let files = paths.into_iter().filter_map(read_file).collect();
        self.attach(files, window, cx);
    }

    /// A paste: pasted images and copied files are attached, and not pasted
    /// as text. Returns whether it was taken. (Text wins over an image on
    /// the clipboard, so a paste with both is text, as usual.)
    fn paste(&mut self, item: &ClipboardItem, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.channel.is_none() || self.editing.editing().is_some() {
            return false;
        }
        let mut files = Vec::new();
        for entry in item.entries() {
            match entry {
                // Copied files come with their paths as text too: attach them.
                ClipboardEntry::ExternalPaths(paths) => {
                    files.extend(paths.paths().iter().cloned().filter_map(read_file));
                }
                ClipboardEntry::Image(image) => {
                    let saved = save_pasted_image(&pasted_dir(), image.format.extension(), &image.bytes)
                        .map_err(|err| {
                            log::warn!("couldn't save a pasted image: {err}");
                            "Couldn't attach the pasted image.".to_string()
                        });
                    files.push(saved);
                }
                ClipboardEntry::String(_) => {}
            }
        }
        if files.is_empty() {
            return false;
        }
        self.attach(files, window, cx);
        true
    }

    /// Add `files` to the tray. Refusals (and files that couldn't be read)
    /// are toasted, once per reason.
    fn attach(
        &mut self,
        files: Vec<Result<DraftAttachment, String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.channel.is_none() || self.editing.editing().is_some() {
            return;
        }
        let mut refusals: Vec<String> = Vec::new();
        for file in files {
            let refusal = match file {
                Ok(file) => match self.draft.add_attachment(file, &self.limits) {
                    Ok(()) => continue,
                    Err(err) => err.to_string(),
                },
                Err(text) => text,
            };
            if !refusals.contains(&refusal) {
                refusals.push(refusal);
            }
        }
        for text in refusals {
            window.push_notification(Notification::warning(text), cx);
        }
        self.focus(window, cx);
        cx.notify();
    }

    /// Replace the box's text, with the cursor at its end. Mentions,
    /// channel links and custom emoji in it become chips.
    fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.with_chips(text);
        self.completion = None;
        self.input.update(cx, |input, cx| {
            input.set_value(content, window, cx);
            input.set_cursor_position(end_of(text), window, cx);
        });
    }

    /// `text` with a chip for each mention in it, labelled from what's been
    /// learnt (see [`Self::learn_labels`]).
    fn with_chips(&self, text: &str) -> InputContent {
        let mut content = InputContent::new(text.to_string());
        for (range, token) in mention_spans(text) {
            let label = self
                .labels
                .get(&token.token_id())
                .cloned()
                .unwrap_or_else(|| unknown_label(&token));
            let chip = InlineToken::new(token.token_id(), token.wire_text()).with_label(label);
            match content.clone().with_token(range, chip) {
                Ok(with) => content = with,
                Err(err) => log::debug!("no chip for {token:?}: {err}"),
            }
        }
        content
    }

    /// Remember labels for the mentions in `text`.
    fn learn_labels(&mut self, text: &str, labels: Labeler) {
        for (_, token) in mention_spans(text) {
            self.labels.insert(token.token_id(), labels(&token));
        }
    }

    /// Open, update or close the suggestion list for what's at the cursor.
    /// Nothing's suggested while an input method is composing.
    fn update_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let composing = self
            .input
            .update(cx, |input, cx| input.marked_text_range(window, cx).is_some());
        let query = (!composing && self.channel.is_some())
            .then(|| {
                let input = self.input.read(cx);
                completion_query(&input.value(), input.cursor())
            })
            .flatten();
        match query {
            None => self.completion = None,
            Some(query) => {
                if self.completion.as_ref().is_some_and(|c| c.query == query) {
                    return;
                }
                self.completion = Some(Completion {
                    query: query.clone(),
                    suggestions: Vec::new(),
                    loading: false,
                    selected: 0,
                });
                cx.emit(ComposerEvent::Complete(query));
            }
        }
    }

    /// The suggestions for `query` (ignored if it's no longer what's being
    /// completed). `loading`: more are on the way.
    pub fn set_suggestions(
        &mut self,
        query: &Query,
        suggestions: Vec<Suggestion>,
        loading: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(completion) = self.completion.as_mut().filter(|c| c.query == *query) else {
            return;
        };
        // Keep the same row selected if it's still there.
        let selected = completion
            .suggestions
            .get(completion.selected)
            .and_then(|old| suggestions.iter().position(|s| s.key == old.key))
            .unwrap_or(0);
        completion.suggestions = suggestions;
        completion.loading = loading;
        completion.selected = selected;
        cx.notify();
    }

    /// What's being completed right now, if the list is open.
    pub fn completion_query(&self) -> Option<&Query> {
        self.completion.as_ref().map(|c| &c.query)
    }

    /// The list is open (there's something to pick, or it's loading).
    fn completing(&self) -> bool {
        self.completion
            .as_ref()
            .is_some_and(|c| !c.suggestions.is_empty() || c.loading)
    }

    /// Put suggestion `ix` in the box in place of what's being completed.
    fn accept(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(completion) = self.completion.take() else {
            return;
        };
        let Some(suggestion) = completion.suggestions.get(ix) else {
            return;
        };
        let range = completion.query.range.clone();
        match &suggestion.insert {
            Insertion::Token { token, label } => {
                self.labels.insert(token.token_id(), label.clone());
                let chip = InlineToken::new(token.token_id(), token.wire_text()).with_label(label.clone());
                self.input.update(cx, |input, cx| {
                    if let Err(err) = input.replace_range_with_token(range, chip, window, cx) {
                        log::warn!("couldn't insert {token:?}: {err}");
                        return;
                    }
                    input.insert(" ", window, cx);
                });
            }
            Insertion::Text(text) => {
                let text = format!("{text} ");
                self.input.update(cx, |input, cx| {
                    input.set_selected_range(range, cx);
                    input.replace(text, window, cx);
                });
            }
        }
        self.focus(window, cx);
        cx.notify();
    }

    /// Edit the user's message `id`, whose text is `content`, in the box
    /// (its mentions labelled by `labels`). Whatever was being typed (and
    /// replied to) comes back afterwards.
    pub fn start_edit(
        &mut self,
        id: String,
        content: &str,
        labels: Labeler,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Edits change text only: the files are set aside with the rest.
        let current = self.take_draft(cx);
        self.editing.begin(id, current);
        self.hint = None;
        self.learn_labels(content, labels);
        self.set_text(content, window, cx);
        self.focus(window, cx);
        cx.notify();
    }

    /// Stop editing `id` (it's being deleted), putting the draft back.
    pub fn stop_editing(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.editing() == Some(id) {
            self.cancel_edit(window, cx);
        }
    }

    /// Leave edit mode, putting back what was being typed. Returns false if
    /// there was no edit.
    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(stash) = self.editing.end() else {
            return false;
        };
        self.draft.replies = stash.replies;
        self.draft.attachments = stash.attachments;
        self.hint = None;
        self.set_text(&stash.text, window, cx);
        cx.notify();
        true
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// Reply to `target` too, and focus the box to type the reply.
    pub fn add_reply(
        &mut self,
        target: ReplyTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), TooManyReplies> {
        self.draft.add_reply(target)?;
        self.focus(window, cx);
        cx.notify();
        Ok(())
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.channel.is_none() {
            return;
        }
        let text = self.input.read(cx).value();
        if let Some(id) = self.editing.editing().map(str::to_string) {
            match check_send(&text, false) {
                SendCheck::Ready(content) => {
                    self.cancel_edit(window, cx);
                    cx.emit(ComposerEvent::Edit { id, content });
                }
                SendCheck::Empty => {
                    self.hint = Some(EMPTY_EDIT_TEXT.into());
                    cx.notify();
                }
                SendCheck::TooLong(_) => {}
            }
            return;
        }
        // Blank and too-long messages stay in the box (the count says why).
        if let SendCheck::Ready(text) = check_send(&text, !self.draft.attachments.is_empty()) {
            let replies = self.draft.reply_intents();
            self.draft.replies.clear();
            let attachments = std::mem::take(&mut self.draft.attachments);
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
            cx.emit(ComposerEvent::Send {
                text,
                replies,
                attachments,
            });
            cx.notify();
        }
    }

    /// Esc with suggestions open: close them (before anything else Esc does).
    fn on_escape_completion(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        if self.completing() {
            cx.stop_propagation();
            self.completion = None;
            cx.notify();
        }
    }

    /// Enter or Tab with suggestions open: pick the selected one.
    /// (Shift+Enter still adds a new line.)
    fn on_enter(&mut self, enter: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.completing() && !enter.shift {
            self.accept_selected(window, cx);
        }
    }

    fn on_tab(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        if self.completing() {
            self.accept_selected(window, cx);
        }
    }

    fn accept_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if let Some(selected) = self.completion.as_ref().map(|c| c.selected) {
            self.accept(selected, window, cx);
        }
    }

    /// Down with suggestions open: select the next one (wrapping).
    fn on_move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.completing() {
            cx.stop_propagation();
            self.move_selection(1, cx);
        }
    }

    fn move_selection(&mut self, step: isize, cx: &mut Context<Self>) {
        if let Some(c) = self.completion.as_mut().filter(|c| !c.suggestions.is_empty()) {
            let len = c.suggestions.len() as isize;
            c.selected = (c.selected as isize + step).rem_euclid(len) as usize;
            cx.notify();
        }
    }

    /// Esc in the message box: stop editing, or else stop replying.
    fn on_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if self.cancel_edit(window, cx) {
            return;
        }
        if self.draft.replies.is_empty() {
            cx.propagate();
            return;
        }
        self.draft.replies.clear();
        cx.notify();
    }

    /// Up with suggestions open: select the previous one. In an empty box
    /// (not already editing): edit the last message. Otherwise Up moves the
    /// cursor as usual.
    fn on_move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.completing() {
            cx.stop_propagation();
            self.move_selection(-1, cx);
            return;
        }
        if self.editing.editing().is_none() && self.input.read(cx).value().is_empty() {
            cx.stop_propagation();
            cx.emit(ComposerEvent::EditLast);
        }
    }

    /// "Editing message", with how to save or cancel.
    fn render_edit_bar(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        h_flex()
            .gap_2()
            .px_2()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(Icon::new(IconName::Pencil).xsmall())
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child("Editing message"),
            )
            .child("Esc to cancel · Enter to save")
            .child(
                Button::new("edit-cancel")
                    .xsmall()
                    .ghost()
                    .icon(IconName::Close)
                    .tooltip("Cancel (Esc)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_edit(window, cx);
                    })),
            )
    }

    /// "Replying to Name", one line per reply, with the mention toggle and ×.
    fn render_reply_bar(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        v_flex()
            .gap_1()
            .children(self.draft.replies.iter().map(|reply| {
                let (toggle, remove) = (reply.id.clone(), reply.id.clone());
                h_flex()
                    .gap_2()
                    .px_2()
                    .text_xs()
                    .text_color(muted)
                    .child(Icon::new(IconName::Reply).xsmall())
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child("Replying to")
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.foreground)
                                    .child(reply.author.clone()),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("reply-mention-{}", reply.id)))
                            .xsmall()
                            .map(|button| {
                                if reply.mention {
                                    button.primary()
                                } else {
                                    button.ghost()
                                }
                            })
                            .label(if reply.mention { "@ On" } else { "@ Off" })
                            .tooltip(if reply.mention {
                                "They'll be notified. Click to turn off."
                            } else {
                                "They won't be notified. Click to turn on."
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.draft.toggle_mention(&toggle);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("reply-remove-{}", reply.id)))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Close)
                            .tooltip("Don't reply to this (Esc for all)")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.draft.remove_reply(&remove);
                                cx.notify();
                            })),
                    )
            }))
    }
}

/// The file at `path`, ready to attach (with an image's size), or why it
/// can't be. `None` for a folder.
fn read_file(path: PathBuf) -> Option<Result<DraftAttachment, String>> {
    let size = match std::fs::metadata(&path) {
        Ok(meta) if meta.is_file() => meta.len(),
        Ok(_) => return None,
        Err(err) => {
            log::warn!("couldn't read {}: {err}", path.display());
            return Some(Err(format!("Couldn't read {}.", path.display())));
        }
    };
    let file = DraftAttachment::from_path(path, size);
    Some(Ok(if file.kind == FileKind::Image {
        let dimensions = crate::attachment::local_image_size(&file.path);
        file.with_dimensions(dimensions)
    } else {
        file
    }))
}

/// Where pasted images are saved before they're sent: a folder of this
/// process's own under the temporary directory (which macOS clears out).
fn pasted_dir() -> PathBuf {
    std::env::temp_dir().join(format!("rustoat-pasted-{}", std::process::id()))
}

/// Side of a file's thumbnail or icon in the tray.
const TRAY_THUMB: f32 = 36.;

impl Composer {
    /// The attached files under the box: a chip each, with ×.
    fn render_tray(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        h_flex()
            .flex_wrap()
            .gap_2()
            .children(self.draft.attachments.iter().map(|file| {
                let local_id = file.local_id;
                let thumb = match file.kind {
                    FileKind::Image => img(file.path.clone())
                        .size(px(TRAY_THUMB))
                        .flex_none()
                        .rounded(px(4.))
                        .object_fit(ObjectFit::Cover)
                        .with_fallback(|| Icon::new(IconName::FileImage).into_any_element())
                        .into_any_element(),
                    kind => div()
                        .size(px(TRAY_THUMB))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(file_icon(kind)).text_color(theme.muted_foreground))
                        .into_any_element(),
                };
                h_flex()
                    .id(SharedString::from(format!("tray-{local_id}")))
                    .max_w(px(TRAY_CHIP_WIDTH))
                    .gap_2()
                    .p_1()
                    .pr_0p5()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.secondary)
                    .child(thumb)
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(div().text_xs().truncate().child(file.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(crate::attachment::size_label(file.size)),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("tray-remove-{local_id}")))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Close)
                            .tooltip("Remove")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.draft.remove_attachment(local_id);
                                cx.notify();
                            })),
                    )
            }))
    }
}

/// Widest a tray chip gets (long names are cut short).
const TRAY_CHIP_WIDTH: f32 = 220.;

/// The icon for a file that isn't drawn as a picture.
pub fn file_icon(kind: FileKind) -> IconName {
    match kind {
        FileKind::Image => IconName::FileImage,
        FileKind::Video => IconName::FileVideoCamera,
        FileKind::Audio => IconName::FileMusic,
        FileKind::Text => IconName::FileText,
        FileKind::File => IconName::File,
    }
}

/// A chip's label when what it refers to isn't known (yet).
fn unknown_label(token: &TokenRef) -> String {
    match token {
        TokenRef::User(_) => "@Unknown user".into(),
        TokenRef::Channel(_) => "#unknown-channel".into(),
        TokenRef::Emoji(_) => ":emoji:".into(),
    }
}

/// A mention or channel link in the box: a pill in the primary tint.
fn render_chip(chip: &InlineTokenContext, _: &mut Window, cx: &mut App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .items_center()
        .px_1()
        .h(chip.line_height())
        .max_w(chip.available_width())
        .rounded(px(4.))
        .bg(if chip.is_selected() {
            theme.selection
        } else {
            theme.primary.opacity(0.15)
        })
        .text_color(theme.primary)
        .font_weight(FontWeight::MEDIUM)
        .child(div().min_w_0().truncate().child(chip.token().label().clone()))
}

/// Height of a suggestion row.
const SUGGESTION_HEIGHT: f32 = 32.;
/// Avatar (or emoji) size in a suggestion row.
const SUGGESTION_IMAGE: f32 = 20.;

impl Composer {
    /// The suggestion list, just above the box and as wide as it.
    fn render_suggestions(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.theme();
        let completion = self.completion.as_ref();
        let (suggestions, selected, loading) = completion
            .map(|c| (c.suggestions.as_slice(), c.selected, c.loading))
            .unwrap_or_default();
        v_flex()
            .id("suggestions")
            .absolute()
            .bottom_full()
            .left_0()
            .right_0()
            .mb_1()
            .p_1()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_md()
            .occlude()
            .children(suggestions.iter().enumerate().map(|(ix, suggestion)| {
                let picture = match (&suggestion.image, &suggestion.glyph) {
                    (Some(url), _) => img(SharedUri::from(url.clone()))
                        .size(px(SUGGESTION_IMAGE))
                        .flex_none()
                        .rounded_full()
                        .into_any_element(),
                    (None, Some(glyph)) => div()
                        .w(px(SUGGESTION_IMAGE))
                        .flex_none()
                        .text_center()
                        .child(glyph.clone())
                        .into_any_element(),
                    (None, None) => div().w(px(SUGGESTION_IMAGE)).into_any_element(),
                };
                h_flex()
                    .id(SharedString::from(format!("suggestion-{}", suggestion.key)))
                    .h(px(SUGGESTION_HEIGHT))
                    .px_2()
                    .gap_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(ix == selected, |el| {
                        el.bg(theme.accent).text_color(theme.accent_foreground)
                    })
                    .hover(|el| el.bg(theme.accent))
                    // On mouse down, so the box doesn't lose focus first.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.accept(ix, window, cx);
                        }),
                    )
                    .child(picture)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(suggestion.title.clone()),
                    )
                    .when_some(suggestion.detail.clone(), |el, detail| {
                        el.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(detail),
                        )
                    })
            }))
            .when(loading, |el| {
                el.child(
                    div()
                        .h(px(SUGGESTION_HEIGHT))
                        .px_2()
                        .flex()
                        .items_center()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("Loading members…"),
                )
            })
    }
}

/// Where the cursor goes at the end of `text`.
fn end_of(text: &str) -> Position {
    let line = text.matches('\n').count();
    let column = text.rsplit('\n').next().unwrap_or_default().chars().count();
    Position::new(line as u32, column as u32)
}

impl Render for Composer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = char_count(&self.input.read(cx).value());
        let bar = if self.editing.editing().is_some() {
            Some(self.render_edit_bar(cx))
        } else {
            (!self.draft.replies.is_empty()).then(|| self.render_reply_bar(cx))
        };
        let hint = self.hint.clone();
        // Edits change text only, so files can't be added meanwhile.
        let editing = self.editing.editing().is_some();
        let tray = (!self.draft.attachments.is_empty()).then(|| self.render_tray(cx));
        let suggestions = self.completing().then(|| self.render_suggestions(cx));
        let attach = Button::new("attach")
            .ghost()
            .icon(IconName::Paperclip)
            .tooltip("Attach files")
            .disabled(editing)
            .on_click(cx.listener(|this, _, window, cx| this.pick_files(window, cx)));
        let theme = cx.theme();
        v_flex()
            .flex_none()
            .px_4()
            .pt_1()
            .pb_4()
            .gap_1()
            .on_action(cx.listener(Self::on_escape))
            // Before the textarea moves the cursor, sends, or indents.
            .capture_action(cx.listener(Self::on_move_up))
            .capture_action(cx.listener(Self::on_move_down))
            .capture_action(cx.listener(Self::on_enter))
            .capture_action(cx.listener(Self::on_tab))
            .capture_action(cx.listener(Self::on_escape_completion))
            .children(bar)
            .child(
                h_flex()
                    .relative()
                    .items_end()
                    .gap_1()
                    .child(attach)
                    .child(
                        div().flex_1().min_w_0().child(
                            Textarea::new(&self.input)
                                .on_paste({
                                    let this = cx.entity().downgrade();
                                    move |item, window, cx| {
                                        this.update(cx, |this, cx| this.paste(item, window, cx))
                                            .unwrap_or(false)
                                    }
                                })
                                .token(render_chip),
                        ),
                    )
                    .children(suggestions),
            )
            .children(tray)
            .when_some(hint, |el, hint| {
                el.child(div().px_2().text_xs().text_color(theme.danger).child(hint))
            })
            .when_some(count, |el, (chars, over)| {
                el.child(
                    h_flex()
                        .justify_end()
                        .text_xs()
                        .text_color(if over {
                            theme.danger
                        } else {
                            theme.muted_foreground
                        })
                        .child(if over {
                            format!(
                                "{chars} / {MAX_MESSAGE_CHARS} · {} too many to send",
                                chars - MAX_MESSAGE_CHARS
                            )
                        } else {
                            format!("{chars} / {MAX_MESSAGE_CHARS}")
                        }),
                )
            })
    }
}
