//! The settings popover's update control: a state machine over
//! `interval_desktop_core::update`, driven from the GPUI entity. Network and filesystem
//! work run on the tokio runtime (reqwest needs its reactor); results come back over
//! channels, which are safe to await on GPUI's foreground executor.

use gpui::Context;
use interval_desktop_core::update::{self, Check, Release, UpdateError};

use crate::IntervalApp;

#[derive(Clone)]
pub(crate) enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(u8),
    Ready(String),
    Failed(String),
    Blocked(String),
}

impl UpdateState {
    fn from_error(error: UpdateError) -> Self {
        let message = error.to_string();
        match error {
            UpdateError::DevBuild
            | UpdateError::UnsupportedPlatform(_)
            | UpdateError::MissingAsset { .. }
            | UpdateError::NotWritable(..) => Self::Blocked(message),
            _ => Self::Failed(message),
        }
    }

    /// Button label, hint line, and whether the button is clickable.
    pub(crate) fn presentation(&self) -> (String, Option<String>, bool) {
        match self {
            Self::Idle => ("CHECK FOR UPDATES".into(), None, true),
            Self::Checking => ("CHECKING".into(), None, false),
            Self::UpToDate => (
                "CHECK FOR UPDATES".into(),
                Some("interval is up to date.".into()),
                true,
            ),
            Self::Available(release) => (
                format!("INSTALL {}", release.version()),
                Some(format!("Release {} is available.", release.version())),
                true,
            ),
            Self::Downloading(percent) => (format!("DOWNLOADING {percent}%"), None, false),
            Self::Ready(version) => (
                "RESTART".into(),
                Some(format!(
                    "Version {version} is installed and runs from the next launch."
                )),
                true,
            ),
            Self::Failed(message) => ("RETRY".into(), Some(message.clone()), true),
            Self::Blocked(message) => ("OPEN RELEASES".into(), Some(message.clone()), true),
        }
    }
}

impl IntervalApp {
    pub(crate) fn update_button_clicked(&mut self, cx: &mut Context<Self>) {
        match &self.update_state {
            UpdateState::Available(_) => self.install_update(cx),
            UpdateState::Ready(_) => self.restart(cx),
            UpdateState::Blocked(_) => cx.open_url(&update::releases_url()),
            _ => self.check_for_updates(cx),
        }
    }

    pub(crate) fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.update_state,
            UpdateState::Checking | UpdateState::Downloading(_)
        ) {
            return;
        }
        self.update_state = UpdateState::Checking;
        cx.notify();

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tokio.spawn(async move {
            let _ = tx.send(update::check().await);
        });
        cx.spawn(async move |this, cx| {
            let outcome = rx.await;
            let _ = this.update(cx, |app: &mut IntervalApp, cx| {
                app.update_state = match outcome {
                    Ok(Ok(Check::UpToDate)) => UpdateState::UpToDate,
                    Ok(Ok(Check::Available(release))) => UpdateState::Available(release),
                    Ok(Err(error)) => UpdateState::from_error(error),
                    Err(_) => UpdateState::Failed("update check did not finish".into()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Download the release found by [`Self::check_for_updates`] and swap it in.
    pub(crate) fn install_update(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Available(release) = self.update_state.clone() else {
            return;
        };
        let plan = match update::plan() {
            Ok(plan) => plan,
            Err(error) => {
                self.update_state = UpdateState::from_error(error);
                cx.notify();
                return;
            }
        };
        self.update_state = UpdateState::Downloading(0);
        cx.notify();

        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        self.tokio.spawn(async move {
            let staged = update::download(plan, &release, move |percent| {
                let _ = progress_tx.send(percent);
            })
            .await;
            let _ = done_tx.send(staged.and_then(update::apply));
        });
        cx.spawn(async move |this, cx| {
            while let Some(percent) = progress_rx.recv().await {
                let applied = this.update(cx, |app: &mut IntervalApp, cx| {
                    app.update_state = UpdateState::Downloading(percent);
                    cx.notify();
                });
                if applied.is_err() {
                    return;
                }
            }
            let outcome = done_rx.await;
            let _ = this.update(cx, |app: &mut IntervalApp, cx| {
                app.update_state = match outcome {
                    Ok(Ok(version)) => UpdateState::Ready(version),
                    Ok(Err(error)) => UpdateState::from_error(error),
                    Err(_) => UpdateState::Failed("update did not finish".into()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Launch the freshly installed binary at the same path and quit this one.
    fn restart(&mut self, cx: &mut Context<Self>) {
        let launched =
            std::env::current_exe().and_then(|exe| std::process::Command::new(exe).spawn());
        match launched {
            Ok(_) => cx.quit(),
            Err(error) => {
                self.update_state = UpdateState::Failed(format!("could not relaunch: {error}"));
                cx.notify();
            }
        }
    }
}
