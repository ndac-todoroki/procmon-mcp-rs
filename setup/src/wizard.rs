//! The setup's pages and the moves between them, free of any UI code.

/// Which pages the setup walks through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Install,
    Uninstall,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Welcome,
    License,
    Review,
    /// Installing; `done` steps have finished.
    Installing {
        done: usize,
    },
    Installed,
    UninstallConfirm,
    Uninstalling,
    Uninstalled,
    Failed(Failure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The UAC prompt was declined.
    ElevationDeclined,
    /// The elevated worker reported this error.
    Error(String),
}

/// What the UI must do after a move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    StartInstall,
    StartUninstall,
    Close,
}

/// The label on the single primary button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Next,
    Install,
    Finish,
    TryAgain,
    Uninstall,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Primary {
    pub action: Action,
    pub enabled: bool,
    /// Show the shield: selecting it asks for administrator permission.
    pub shield: bool,
}

pub struct Wizard {
    flow: Flow,
    page: Page,
    accepted: bool,
}

impl Wizard {
    pub fn new(flow: Flow) -> Wizard {
        let page = match flow {
            Flow::Install => Page::Welcome,
            Flow::Uninstall => Page::UninstallConfirm,
        };
        Wizard {
            flow,
            page,
            accepted: false,
        }
    }

    pub fn page(&self) -> &Page {
        &self.page
    }

    pub fn accepted(&self) -> bool {
        self.accepted
    }

    pub fn set_accepted(&mut self, accepted: bool) {
        self.accepted = accepted;
    }

    /// Only the pages before anything changes can be left backwards.
    pub fn can_go_back(&self) -> bool {
        matches!(self.page, Page::License | Page::Review)
    }

    pub fn back(&mut self) {
        self.page = match self.page {
            Page::License => Page::Welcome,
            Page::Review => Page::License,
            ref other => other.clone(),
        };
    }

    pub fn primary(&self) -> Primary {
        let (action, enabled, shield) = match self.page {
            Page::Welcome => (Action::Next, true, false),
            Page::License => (Action::Next, self.accepted, false),
            Page::Review => (Action::Install, true, true),
            Page::Installing { .. } => (Action::Next, false, false),
            Page::Installed => (Action::Finish, true, false),
            Page::UninstallConfirm => (Action::Uninstall, true, true),
            Page::Uninstalling => (Action::Close, false, false),
            Page::Uninstalled => (Action::Close, true, false),
            Page::Failed(_) => (Action::TryAgain, true, true),
        };
        Primary {
            action,
            enabled,
            shield,
        }
    }

    pub fn primary_selected(&mut self) -> Option<Effect> {
        if !self.primary().enabled {
            return None;
        }
        match self.page {
            Page::Welcome => {
                self.page = Page::License;
                None
            }
            Page::License => {
                self.page = Page::Review;
                None
            }
            Page::Review | Page::UninstallConfirm | Page::Failed(_) => Some(self.start()),
            Page::Installed | Page::Uninstalled => Some(Effect::Close),
            Page::Installing { .. } | Page::Uninstalling => None,
        }
    }

    fn start(&mut self) -> Effect {
        match self.flow {
            Flow::Install => {
                self.page = Page::Installing { done: 0 };
                Effect::StartInstall
            }
            Flow::Uninstall => {
                self.page = Page::Uninstalling;
                Effect::StartUninstall
            }
        }
    }

    pub fn step_finished(&mut self) {
        if let Page::Installing { done } = &mut self.page {
            *done += 1;
        }
    }

    pub fn worker_finished(&mut self, result: Result<(), Failure>) {
        if !matches!(self.page, Page::Installing { .. } | Page::Uninstalling) {
            return;
        }
        self.page = match (result, self.flow) {
            (Ok(()), Flow::Install) => Page::Installed,
            (Ok(()), Flow::Uninstall) => Page::Uninstalled,
            (Err(failure), _) => Page::Failed(failure),
        };
    }

    /// Position in the install walkthrough's step indicator.
    pub fn step_index(&self) -> usize {
        match self.page {
            Page::Welcome => 0,
            Page::License => 1,
            Page::Review => 2,
            Page::Installing { .. } | Page::Failed(_) => 3,
            Page::Installed => 4,
            Page::UninstallConfirm => 0,
            Page::Uninstalling => 1,
            Page::Uninstalled => 2,
        }
    }

    pub fn flow(&self) -> Flow {
        self.flow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_review() -> Wizard {
        let mut w = Wizard::new(Flow::Install);
        w.primary_selected();
        w.set_accepted(true);
        w.primary_selected();
        w
    }

    fn installing() -> Wizard {
        let mut w = at_review();
        assert_eq!(w.primary_selected(), Some(Effect::StartInstall));
        w
    }

    #[test]
    fn install_starts_on_the_welcome_page_without_a_way_back() {
        let w = Wizard::new(Flow::Install);
        assert_eq!(w.page(), &Page::Welcome);
        assert!(!w.can_go_back());
        assert_eq!(
            w.primary(),
            Primary {
                action: Action::Next,
                enabled: true,
                shield: false
            }
        );
    }

    #[test]
    fn next_leads_from_welcome_to_license() {
        let mut w = Wizard::new(Flow::Install);
        assert_eq!(w.primary_selected(), None);
        assert_eq!(w.page(), &Page::License);
        assert!(w.can_go_back());
    }

    #[test]
    fn license_page_holds_until_the_terms_are_accepted() {
        let mut w = Wizard::new(Flow::Install);
        w.primary_selected();
        assert!(!w.primary().enabled);
        w.primary_selected();
        assert_eq!(w.page(), &Page::License);

        w.set_accepted(true);
        assert!(w.primary().enabled);
        w.primary_selected();
        assert_eq!(w.page(), &Page::Review);
    }

    #[test]
    fn withdrawing_acceptance_on_the_way_back_blocks_again() {
        let mut w = at_review();
        w.back();
        assert_eq!(w.page(), &Page::License);
        w.set_accepted(false);
        assert!(!w.primary().enabled);
    }

    #[test]
    fn back_retraces_review_license_welcome() {
        let mut w = at_review();
        w.back();
        assert_eq!(w.page(), &Page::License);
        w.back();
        assert_eq!(w.page(), &Page::Welcome);
        w.back();
        assert_eq!(w.page(), &Page::Welcome);
    }

    #[test]
    fn review_offers_install_with_the_shield_and_starts_it() {
        let mut w = at_review();
        assert_eq!(
            w.primary(),
            Primary {
                action: Action::Install,
                enabled: true,
                shield: true
            }
        );
        assert_eq!(w.primary_selected(), Some(Effect::StartInstall));
        assert_eq!(w.page(), &Page::Installing { done: 0 });
    }

    #[test]
    fn installing_cannot_be_left_and_counts_finished_steps() {
        let mut w = installing();
        assert!(!w.can_go_back());
        assert!(!w.primary().enabled);
        assert_eq!(w.primary_selected(), None);
        w.step_finished();
        w.step_finished();
        assert_eq!(w.page(), &Page::Installing { done: 2 });
    }

    #[test]
    fn success_shows_the_installed_page_whose_button_closes() {
        let mut w = installing();
        w.worker_finished(Ok(()));
        assert_eq!(w.page(), &Page::Installed);
        assert!(!w.can_go_back());
        assert_eq!(w.primary().action, Action::Finish);
        assert_eq!(w.primary_selected(), Some(Effect::Close));
    }

    #[test]
    fn failure_is_shown_and_can_be_retried() {
        let mut w = installing();
        w.worker_finished(Err(Failure::Error("cannot replace procmon-mcp.exe".into())));
        assert_eq!(
            w.page(),
            &Page::Failed(Failure::Error("cannot replace procmon-mcp.exe".into()))
        );
        assert!(!w.can_go_back());
        assert_eq!(
            w.primary(),
            Primary {
                action: Action::TryAgain,
                enabled: true,
                shield: true
            }
        );
        assert_eq!(w.primary_selected(), Some(Effect::StartInstall));
        assert_eq!(w.page(), &Page::Installing { done: 0 });
    }

    #[test]
    fn steps_reported_after_the_end_are_ignored() {
        let mut w = installing();
        w.worker_finished(Ok(()));
        w.step_finished();
        assert_eq!(w.page(), &Page::Installed);
    }

    #[test]
    fn step_indicator_positions() {
        let mut w = Wizard::new(Flow::Install);
        let mut seen = vec![w.step_index()];
        w.primary_selected();
        seen.push(w.step_index());
        w.set_accepted(true);
        w.primary_selected();
        seen.push(w.step_index());
        w.primary_selected();
        seen.push(w.step_index());
        w.worker_finished(Err(Failure::ElevationDeclined));
        seen.push(w.step_index());
        w.primary_selected();
        w.worker_finished(Ok(()));
        seen.push(w.step_index());
        assert_eq!(seen, [0, 1, 2, 3, 3, 4]);
    }

    #[test]
    fn uninstall_confirms_then_runs_with_the_shield() {
        let mut w = Wizard::new(Flow::Uninstall);
        assert_eq!(w.page(), &Page::UninstallConfirm);
        assert!(!w.can_go_back());
        assert_eq!(
            w.primary(),
            Primary {
                action: Action::Uninstall,
                enabled: true,
                shield: true
            }
        );
        assert_eq!(w.primary_selected(), Some(Effect::StartUninstall));
        assert_eq!(w.page(), &Page::Uninstalling);
        assert!(!w.primary().enabled);
    }

    #[test]
    fn uninstall_ends_removed_or_failed_with_retry() {
        let mut w = Wizard::new(Flow::Uninstall);
        w.primary_selected();
        w.worker_finished(Err(Failure::ElevationDeclined));
        assert_eq!(w.primary().action, Action::TryAgain);
        assert_eq!(w.primary_selected(), Some(Effect::StartUninstall));
        w.worker_finished(Ok(()));
        assert_eq!(w.page(), &Page::Uninstalled);
        assert_eq!(w.primary().action, Action::Close);
        assert_eq!(w.primary_selected(), Some(Effect::Close));
    }
}
