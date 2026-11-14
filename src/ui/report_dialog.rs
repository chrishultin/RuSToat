//! "Tell us what's wrong with this message/user/server": report something
//! to Stoat's moderators, with a reason and some detail (see
//! [`crate::reports`] for the reasons, the request and the rules).

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::*;

use crate::api::StoatClient;
use crate::reports::{DETAIL_MAX, Reason, ReportTarget, can_send, report_body, report_error_text};
use crate::runtime::spawn_io;
use crate::ui::profile_editor::field;

/// The dialog's width.
const REPORT_DIALOG_WIDTH: f32 = 440.;
/// Rows the detail field grows between before it scrolls.
const DETAIL_ROWS: (usize, usize) = (3, 8);
/// The tallest the reason menu grows before it scrolls.
const REASON_MENU_HEIGHT: f32 = 360.;

/// Open the report dialog for `target`, in `window`. `subject` says what's
/// reported: a name, or the start of a message.
pub fn open_report_dialog(
    client: StoatClient,
    target: ReportTarget,
    subject: SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    let title = SharedString::from(target.title());
    let dialog = cx.new(|cx| ReportDialog::new(client, target, subject, window, cx));
    window.open_dialog(cx, move |modal, _, _| {
        modal
            .title(title.clone())
            .w(px(REPORT_DIALOG_WIDTH))
            .child(dialog.clone())
    });
}

/// The report dialog's content.
pub struct ReportDialog {
    client: StoatClient,
    target: ReportTarget,
    subject: SharedString,
    reason: Option<Reason>,
    detail: Entity<TextareaState>,
    sending: bool,
    _detail: Subscription,
}

impl ReportDialog {
    fn new(
        client: StoatClient,
        target: ReportTarget,
        subject: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let detail = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(DETAIL_ROWS.0, DETAIL_ROWS.1)
                .placeholder("What happened?")
        });
        let subscription = cx.subscribe(&detail, |_, _, _: &InputEvent, cx| cx.notify());
        Self {
            client,
            target,
            subject,
            reason: None,
            detail,
            sending: false,
            _detail: subscription,
        }
    }

    /// Send the report. Stoat's thanks close the dialog; a refusal toasts
    /// and keeps it open.
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let detail = self.detail.read(cx).value().to_string();
        if self.sending || !can_send(self.reason.as_ref(), &detail) {
            return;
        }
        let Some(body) = self
            .reason
            .as_ref()
            .and_then(|reason| report_body(&self.target, reason, &detail))
        else {
            return;
        };
        self.sending = true;
        cx.notify();
        let request = spawn_io({
            let client = self.client.clone();
            async move { client.report(&body).await }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            this.update_in(cx, |this, window, cx| {
                this.sending = false;
                match result {
                    Ok(Ok(())) => {
                        log::info!("sent a report");
                        window.close_dialog(cx);
                        window.push_notification(
                            Notification::success("Thanks, your report was sent."),
                            cx,
                        );
                    }
                    Ok(Err(err)) => {
                        log::warn!("report refused: {err:?}");
                        window.push_notification(Notification::error(report_error_text(&err)), cx);
                    }
                    Err(err) => {
                        log::warn!("report task failed: {err:?}");
                        window.push_notification(
                            Notification::error("Couldn't send the report."),
                            cx,
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The reasons, the chosen one ticked.
    fn reason_menu(
        &self,
        cx: &Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let (entity, reasons, chosen) = (
            cx.entity().downgrade(),
            self.target.reasons(),
            self.reason.clone(),
        );
        move |menu, _, _| {
            let menu = menu.scrollable(true).max_h(px(REASON_MENU_HEIGHT));
            reasons.iter().fold(menu, |menu, reason| {
                let (entity, picked) = (entity.clone(), reason.clone());
                menu.item(
                    PopupMenuItem::new(reason.label())
                        .checked(chosen.as_ref() == Some(reason))
                        .on_click(move |_, _, cx| {
                            entity
                                .update(cx, |this, cx| {
                                    this.reason = Some(picked.clone());
                                    cx.notify();
                                })
                                .ok();
                        }),
                )
            })
        }
    }
}

impl Render for ReportDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let detail = self.detail.read(cx).value().to_string();
        let count = detail.trim().chars().count();
        let other = self.reason.as_ref().is_some_and(Reason::is_other);
        let problem = (other && count == 0).then(|| "Tell us what's wrong.".to_string());
        let reason_label = self
            .reason
            .as_ref()
            .map_or("Please select a reason", Reason::label);
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .line_clamp(3)
                    .child(self.subject.clone()),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Reason for report"),
                    )
                    .child(
                        Button::new("report-reason")
                            .outline()
                            .small()
                            .w_full()
                            .label(reason_label)
                            .icon(IconName::ChevronDown)
                            .disabled(self.sending)
                            .dropdown_menu(self.reason_menu(cx)),
                    ),
            )
            .child(field(
                "Give us some detail",
                Textarea::new(&self.detail)
                    .disabled(self.sending)
                    .into_any_element(),
                count,
                DETAIL_MAX,
                problem,
                cx,
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Reports go to this Stoat instance's moderators."),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("report-cancel")
                            .outline()
                            .label("Cancel")
                            .disabled(self.sending)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("report-send")
                            .danger()
                            .label("Report")
                            .loading(self.sending)
                            .disabled(self.sending || !can_send(self.reason.as_ref(), &detail))
                            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
                    ),
            )
    }
}
