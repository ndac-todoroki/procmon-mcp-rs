//! The setup window (WinUI 3 through windows-reactor). Page logic lives in `wizard`;
//! this file only draws the current page and forwards clicks.

use std::io::{BufRead, BufReader};
use std::sync::Arc;

use procmon_mcp::install::{self, EULA_URL, SYSINTERNALS_LIVE};
use procmon_mcp::layout::Layout;
use windows_reactor::*;

use crate::protocol::{Event, PipeId, Task, worker_args};
use crate::text::{Lang, Text};
use crate::win::{self, LaunchError, PipeServer};
use crate::wizard::{Action, Effect, Failure, Flow, Page, Wizard};

const MONO: &str = "Cascadia Mono, Consolas";

// Segoe Fluent Icons glyphs.
const ICON_FOLDER: &str = "\u{E8B7}";
const ICON_DOWNLOAD: &str = "\u{E896}";
const ICON_TASK: &str = "\u{E823}";
const ICON_APPS: &str = "\u{E71D}";
const ICON_SHIELD: &str = "\u{EA18}";
const ICON_CHECK: &str = "\u{E73E}";
const ICON_COPY: &str = "\u{E8C8}";
const ICON_DONE: &str = "\u{E930}";
const ICON_SEARCH: &str = "\u{E721}";
const ICON_CHART: &str = "\u{E9D9}";
const ICON_RESTORE: &str = "\u{E777}";

/// What the window is started with.
#[derive(Clone, PartialEq)]
pub struct Input {
    pub flow: Flow,
    pub lang: Lang,
}

/// Opens the window and returns when it closes. Fails when WinUI cannot start, most
/// likely because the Windows App Runtime is missing.
pub fn run(flow: Flow, lang: Lang) -> Result<(), String> {
    App::run_component::<Setup>(Input { flow, lang }).map_err(|e| e.to_string())
}

struct Setup {
    wizard: Wizard,
    layout: Option<Layout>,
    already_installed: bool,
    copied: bool,
    in_use: usize,
}

#[derive(Clone)]
enum Msg {
    Primary,
    Back,
    Accept(bool),
    Copy,
    /// The window handle, to own the UAC prompt.
    Launch(isize),
    Worker(Event),
    Declined,
}

impl Component for Setup {
    type Input = Input;
    type Message = Msg;

    fn create(input: &Input, _: &ComponentContext<Self>) -> Self {
        let layout = Layout::system().ok();
        let already_installed = layout.as_ref().is_some_and(|l| l.server().is_file());
        Setup {
            wizard: Wizard::new(input.flow),
            layout,
            already_installed,
            copied: false,
            in_use: 0,
        }
    }

    fn update(&mut self, message: Msg, context: &ComponentContext<Self>) {
        match message {
            Msg::Primary => match self.wizard.primary_selected() {
                Some(Effect::StartInstall | Effect::StartUninstall) => {
                    let _ = context.run_window(|window| Msg::Launch(window.as_raw() as isize));
                }
                Some(Effect::Close) => {
                    let _ = context.close_window();
                }
                None => {}
            },
            Msg::Back => self.wizard.back(),
            Msg::Accept(accepted) => self.wizard.set_accepted(accepted),
            Msg::Copy => {
                if let Some(layout) = &self.layout {
                    self.copied =
                        win::copy_to_clipboard(&install::claude_desktop_entry(&layout.server()))
                            .is_ok();
                }
            }
            Msg::Launch(window) => {
                let task = match self.wizard.flow() {
                    Flow::Install => Task::Install,
                    Flow::Uninstall => Task::Uninstall,
                };
                let completion = context.completion();
                std::thread::spawn(move || run_worker(task, window, completion));
            }
            Msg::Worker(Event::StepDone(_)) => self.wizard.step_finished(),
            Msg::Worker(Event::InUse(n)) => self.in_use = n,
            Msg::Worker(Event::Finished) => self.wizard.worker_finished(Ok(())),
            Msg::Worker(Event::Failed(message)) => {
                self.wizard.worker_finished(Err(Failure::Error(message)))
            }
            Msg::Declined => self.wizard.worker_finished(Err(Failure::ElevationDeclined)),
        }
    }

    fn view(&self, input: &Input, context: &mut ViewContext<Self>) -> View {
        let t = input.lang.text();
        context.window_visuals(
            WindowVisuals::new()
                .client_size(640.0, 580.0)
                .backdrop(WindowBackdrop::Mica)
                .maximizable(false),
        );
        context.window_title(t.window_title);
        let page = match self.wizard.page() {
            Page::Welcome => self.welcome(t),
            Page::License => self.license(t, context),
            Page::Review => self.review(t),
            Page::Installing { done } => self.installing(t, *done),
            Page::Installed => self.installed(t, context),
            Page::UninstallConfirm => self.uninstall_confirm(t),
            Page::Uninstalling => self.uninstalling(t),
            Page::Uninstalled => self.uninstalled(t),
            Page::Failed(failure) => self.failed(t, failure),
        };
        let can_go_back = self.wizard.can_go_back();
        let title_bar = TitleBar::new()
            .title(t.window_title)
            .is_back_button_visible(can_go_back)
            .is_back_button_enabled(can_go_back)
            .on_back_requested(context.message(Msg::Back));
        let header: View = match self.wizard.flow() {
            Flow::Install => self.stepper(t).into(),
            Flow::Uninstall => TextBlock::new().text("").into(),
        };
        let content = Grid::new()
            .grid_row(1)
            .rows([GridLength::Auto, GridLength::STAR, GridLength::Auto])
            .row_spacing(20.0)
            .margin(Thickness::new(32.0, 12.0, 32.0, 24.0))
            .children((
                header,
                ScrollViewer::new().grid_row(1).content(
                    Border::new()
                        .padding(Thickness::new(0.0, 0.0, 16.0, 0.0))
                        .content(page),
                ),
                self.primary_button(t, context).grid_row(2),
            ));
        Grid::new()
            .rows([GridLength::Auto, GridLength::STAR])
            .children((title_bar, content))
            .into()
    }
}

/// Starts the elevated worker and forwards its events to the window, on its own thread.
fn run_worker(task: Task, window: isize, completion: ComponentCompletion<Msg>) {
    let send = |message: Msg| {
        let _ = completion.clone().complete(message);
    };
    let fail = |message: String| send(Msg::Worker(Event::Failed(message)));
    let (id, server) = match PipeId::new().and_then(|id| Ok((id.clone(), PipeServer::create(&id)?)))
    {
        Ok(pair) => pair,
        Err(e) => return fail(format!("cannot prepare the setup's pipe: {e}")),
    };
    let process = match win::launch_elevated(window, &worker_args(task, &id)) {
        Ok(process) => Arc::new(process),
        Err(LaunchError::Declined) => return send(Msg::Declined),
        Err(LaunchError::Other(e)) => return fail(format!("cannot start the setup elevated: {e}")),
    };
    let pipe = match server.accept(Arc::clone(&process)) {
        Ok(pipe) => pipe,
        Err(e) => return fail(e.to_string()),
    };
    for line in BufReader::new(pipe).lines() {
        let Ok(line) = line else { break };
        if let Some(event) = Event::from_line(&line) {
            let last = matches!(event, Event::Finished | Event::Failed(_));
            send(Msg::Worker(event));
            if last {
                return;
            }
        }
    }
    let code = win::exit_code(&process).unwrap_or(u32::MAX);
    fail(format!(
        "the elevated setup stopped unexpectedly (exit code {code})"
    ));
}

fn heading(text: &str) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_size(28.0)
        .font_weight(FontWeight::SEMI_BOLD)
        .text_wrapping(TextWrapping::Wrap)
        .automation_heading_level(AutomationHeadingLevel::Level1)
}

fn body(text: &str) -> TextBlock {
    TextBlock::new()
        .text(text)
        .text_wrapping(TextWrapping::Wrap)
}

fn secondary(text: &str) -> TextBlock {
    body(text).opacity(0.75)
}

/// Paths, URLs and identifiers: never translated, always monospace.
fn mono(text: &str) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_family(MONO)
        .is_text_selection_enabled(true)
        .text_wrapping(TextWrapping::Wrap)
}

fn code(text: &str) -> Border {
    Border::new()
        .background(ThemeBrush::CardBackground)
        .border_brush(ThemeBrush::CardStroke)
        .border_thickness(Thickness::uniform(1.0))
        .corner_radius(CornerRadius::uniform(4.0))
        .padding(Thickness::xy(12.0, 8.0))
        .content(mono(text).font_size(13.0))
}

fn icon(glyph: &str, size: f64) -> FontIcon {
    FontIcon::new().glyph(glyph).font_size(size)
}

/// An icon on the left; a title and a detail line on the right.
fn item(glyph: &str, title: &str, detail: TextBlock) -> Grid {
    Grid::new()
        .columns([GridLength::Pixel(36.0), GridLength::STAR])
        .children((
            icon(glyph, 20.0)
                .vertical_alignment(VerticalAlignment::Top)
                .margin(Thickness::new(0.0, 2.0, 0.0, 0.0)),
            StackPanel::new().spacing(2.0).grid_column(1).children((
                TextBlock::new()
                    .text(title)
                    .font_weight(FontWeight::SEMI_BOLD)
                    .text_wrapping(TextWrapping::Wrap),
                detail.opacity(0.8),
            )),
        ))
}

fn label_with_icon(glyph: &str, label: &str) -> StackPanel {
    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(8.0)
        .children((icon(glyph, 14.0), TextBlock::new().text(label)))
}

fn info(severity: InfoBarSeverity, title: &str, message: &str) -> InfoBar {
    InfoBar::new()
        .is_open(true)
        .is_closable(false)
        .severity(severity)
        .title(title)
        .message(message)
}

impl Setup {
    fn path(&self, path: impl Fn(&Layout) -> std::path::PathBuf) -> String {
        self.layout
            .as_ref()
            .map(|l| path(l).display().to_string())
            .unwrap_or_default()
    }

    fn stepper(&self, t: &Text) -> StackPanel {
        let names = [
            t.step_welcome,
            t.step_license,
            t.step_review,
            t.step_install,
            t.step_done,
        ];
        let current = self.wizard.step_index();
        let pips: Vec<View> = (0..names.len())
            .map(|i| {
                let (width, brush) = match i.cmp(&current) {
                    std::cmp::Ordering::Equal => (20.0, ThemeBrush::Accent),
                    std::cmp::Ordering::Less => (8.0, ThemeBrush::Accent),
                    std::cmp::Ordering::Greater => (8.0, ThemeBrush::PrimaryText),
                };
                Border::new()
                    .width(width)
                    .height(8.0)
                    .corner_radius(CornerRadius::uniform(4.0))
                    .background(brush)
                    .opacity(if i > current { 0.3 } else { 1.0 })
                    .vertical_alignment(VerticalAlignment::Center)
                    .into()
            })
            .collect();
        let label = t
            .step_of
            .replace("{n}", &(current + 1).to_string())
            .replace("{total}", &names.len().to_string())
            .replace("{name}", names[current]);
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(12.0)
            .children((
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(6.0)
                    .vertical_alignment(VerticalAlignment::Center)
                    .children(pips),
                secondary(&label).vertical_alignment(VerticalAlignment::Center),
            ))
    }

    fn welcome(&self, t: &Text) -> View {
        let mut children: Vec<View> = vec![
            heading(t.welcome_heading).into(),
            body(t.welcome_body).into(),
        ];
        if self.already_installed {
            children.push(info(InfoBarSeverity::Informational, "", t.already_installed).into());
        }
        children.push(
            StackPanel::new()
                .spacing(14.0)
                .margin(Thickness::new(0.0, 8.0, 0.0, 0.0))
                .children((
                    item(
                        ICON_SEARCH,
                        t.feature_capture,
                        body(t.feature_capture_detail),
                    ),
                    item(
                        ICON_CHART,
                        t.feature_summary,
                        body(t.feature_summary_detail),
                    ),
                    item(
                        ICON_RESTORE,
                        t.feature_settings,
                        body(t.feature_settings_detail),
                    ),
                ))
                .into(),
        );
        StackPanel::new().spacing(16.0).children(children).into()
    }

    fn license(&self, t: &Text, context: &ViewContext<Self>) -> View {
        let source = format!(
            "https://{SYSINTERNALS_LIVE}{}",
            install::procmon_download_path(std::env::consts::ARCH)
        );
        StackPanel::new()
            .spacing(16.0)
            .children((
                heading(t.heading_procmon),
                body(t.license_body),
                code(&format!(
                    "{source}\n\u{2192} {}",
                    self.path(Layout::procmon)
                )),
                secondary(t.license_signature),
                info(
                    InfoBarSeverity::Informational,
                    t.license_title,
                    t.license_message,
                ),
                HyperlinkButton::new()
                    .content(t.license_link)
                    .navigate_uri(EULA_URL)
                    .expect("EULA_URL is a valid URI"),
                CheckBox::new()
                    .content(t.license_accept)
                    .is_checked(Some(self.wizard.accepted()))
                    .on_is_checked_changed(
                        context.callback(|v: Option<bool>| Msg::Accept(v == Some(true))),
                    ),
            ))
            .into()
    }

    fn review(&self, t: &Text) -> View {
        let small = |text: &str| mono(text).font_size(12.0);
        StackPanel::new()
            .spacing(16.0)
            .children((
                heading(t.review_heading),
                body(t.review_body),
                StackPanel::new().spacing(14.0).children((
                    item(
                        ICON_FOLDER,
                        t.review_folder,
                        small(&format!(
                            "{}\n{}\\{{jobs, out}}",
                            self.path(|l| l.programs.clone()),
                            self.path(|l| l.data.clone())
                        )),
                    ),
                    item(
                        ICON_DOWNLOAD,
                        t.review_download,
                        small(&format!(
                            "{SYSINTERNALS_LIVE} \u{2192} {}",
                            self.path(Layout::procmon)
                        )),
                    ),
                    item(
                        ICON_TASK,
                        t.review_task,
                        small(procmon_mcp::task::TASK_NAME),
                    ),
                    item(
                        ICON_APPS,
                        t.review_uninstaller,
                        small(&self.path(Layout::setup)),
                    ),
                )),
                info(InfoBarSeverity::Warning, t.admin_title, t.admin_message),
            ))
            .into()
    }

    fn installing(&self, t: &Text, done: usize) -> View {
        let labels = [
            t.progress_folder,
            t.progress_copy,
            t.progress_download,
            t.progress_register,
        ];
        let rows: Vec<View> = labels
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let marker: View = if i < done {
                    icon(ICON_CHECK, 16.0).foreground(ThemeBrush::Accent).into()
                } else if i == done {
                    ProgressRing::new()
                        .is_active(true)
                        .width(16.0)
                        .height(16.0)
                        .into()
                } else {
                    TextBlock::new().text("").into()
                };
                Grid::new()
                    .columns([GridLength::Pixel(32.0), GridLength::STAR])
                    .children((
                        marker,
                        body(label)
                            .grid_column(1)
                            .opacity(if i <= done { 1.0 } else { 0.5 }),
                    ))
                    .into()
            })
            .collect();
        StackPanel::new()
            .spacing(16.0)
            .children((
                heading(t.installing_heading),
                ProgressBar::new()
                    .minimum(0.0)
                    .maximum(labels.len() as f64)
                    .value(done as f64),
                StackPanel::new().spacing(12.0).children(rows),
            ))
            .into()
    }

    fn installed(&self, t: &Text, context: &ViewContext<Self>) -> View {
        let label = if self.copied { t.copied } else { t.copy };
        let entry = install::claude_desktop_entry(
            &self.layout.as_ref().map(Layout::server).unwrap_or_default(),
        );
        StackPanel::new()
            .spacing(16.0)
            .children((
                done_heading(t.done_heading),
                body(t.done_body),
                code(&entry),
                Button::new()
                    .content(label_with_icon(
                        if self.copied { ICON_CHECK } else { ICON_COPY },
                        label,
                    ))
                    .automation_name(label)
                    .on_click(context.message(Msg::Copy)),
                secondary(t.uninstall_note),
            ))
            .into()
    }

    fn uninstall_confirm(&self, t: &Text) -> View {
        StackPanel::new()
            .spacing(16.0)
            .children((
                heading(t.uninstall_heading),
                body(t.uninstall_body),
                code(&format!(
                    "{}\n{}",
                    self.path(|l| l.programs.clone()),
                    self.path(|l| l.data.clone())
                )),
            ))
            .into()
    }

    fn uninstalling(&self, t: &Text) -> View {
        StackPanel::new()
            .spacing(16.0)
            .children((
                heading(t.uninstalling_heading),
                ProgressBar::new().is_indeterminate(true),
            ))
            .into()
    }

    fn uninstalled(&self, t: &Text) -> View {
        let mut children: Vec<View> = vec![
            done_heading(t.uninstalled_heading).into(),
            body(t.uninstalled_body).into(),
        ];
        if self.in_use > 0 {
            children.push(secondary(t.in_use_note).into());
        }
        StackPanel::new().spacing(16.0).children(children).into()
    }

    fn failed(&self, t: &Text, failure: &Failure) -> View {
        let title = match self.wizard.flow() {
            Flow::Install => t.failed_heading,
            Flow::Uninstall => t.uninstall_failed_heading,
        };
        let detail: View = match failure {
            Failure::ElevationDeclined => body(t.elevation_declined).into(),
            Failure::Error(message) => code(message).into(),
        };
        StackPanel::new()
            .spacing(16.0)
            .children((heading(title), detail))
            .into()
    }

    /// The single primary action, bottom right.
    fn primary_button(&self, t: &Text, context: &ViewContext<Self>) -> Button {
        let primary = self.wizard.primary();
        let label = match primary.action {
            Action::Next => t.next,
            Action::Install => t.install,
            Action::Finish => t.finish,
            Action::TryAgain => t.try_again,
            Action::Uninstall => t.uninstall,
            Action::Close => t.close,
        };
        let content: View = if primary.shield {
            label_with_icon(ICON_SHIELD, label).into()
        } else {
            label.into()
        };
        Button::new()
            .content(content)
            .automation_name(label)
            .style(ButtonStyle::Accent)
            .min_width(120.0)
            .is_enabled(primary.enabled)
            .horizontal_alignment(HorizontalAlignment::Right)
            .on_click(context.message(Msg::Primary))
    }
}

fn done_heading(text: &str) -> StackPanel {
    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(12.0)
        .children((
            icon(ICON_DONE, 32.0).foreground(ThemeBrush::Accent),
            heading(text),
        ))
}
