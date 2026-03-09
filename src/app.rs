// SPDX-License-Identifier: MPL-2.0
use crate::config::Config;
use crate::fl;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::Subscription;
use cosmic::iced::futures::channel::mpsc::Sender;
use cosmic::{Action, prelude::*};
use futures_util::SinkExt;
use notify_rust::{Notification, Timeout};
use tokio::process::Command;
use tokio::time::{Duration, sleep};

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
#[derive(Default)]
pub struct AppModel {
    is_updating: bool,
    has_updates: bool,
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// Configuration data that persists between application runs.
    config: Config,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    ButtonPressed,
    UpdateConfig(Config),
    UpdateHasUpdates(bool),
    UpdateIsUpdating(bool),
}

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = ();

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = "com.github.ErenYeager1709.update-applet";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        // Construct the app model with the runtime's core.
        let app = AppModel {
            is_updating: false,
            has_updates: false,
            core,
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => {
                        // for why in errors {
                        //     tracing::error!(%why, "error loading app config");
                        // }

                        config
                    }
                })
                .unwrap_or_default(),
        };

        (app, Task::none())
    }

    /// Describes the interface based on the current state of the application model.
    ///
    /// The applet's button in the panel will be drawn using the main view method.
    /// This view should emit messages to toggle the applet's popup window, which will
    /// be drawn using the `view_window` method.
    fn view(&self) -> Element<'_, Self::Message> {
        let icon = match (self.is_updating, self.has_updates) {
            (false, true) => "document-save-symbolic",
            (true, _) => "system-run-symbolic",
            (false, false) => "object-select-symbolic",
        };

        self.core
            .applet
            .icon_button(icon)
            .on_press(Message::ButtonPressed)
            .into()
    }

    /// Register subscriptions for this application.
    ///
    /// Subscriptions are long-lived async tasks running in the background which
    /// emit messages to the application through a channel. They may be conditionally
    /// activated by selectively appending to the subscription batch, and will
    /// continue to execute for the duration that they remain in the batch.
    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch(vec![
            Subscription::run(|| {
                cosmic::iced::stream::channel(4, |mut channel| async move {
                    loop {
                        let mut is_update_available = false;
                        match Command::new("dnf").arg("check-update").output().await {
                            Ok(result) => {
                                println!("Update-Check: {:?}", result.status.code());

                                if matches!(result.status.code(), Some(100)) {
                                    is_update_available = true;
                                }
                            }
                            Err(err) => {
                                Notification::new()
                                    .summary(&fl!("dnf-notification-header"))
                                    .body(&err.to_string())
                                    .timeout(Timeout::Milliseconds(5000))
                                    .show_async()
                                    .await
                                    .unwrap();
                            }
                        }

                        match Command::new("flatpak")
                            .args(["remote-ls", "--updates"])
                            .output()
                            .await
                        {
                            Ok(result) => {
                                let stdout = String::from_utf8_lossy(&result.stdout);
                                let count = stdout.lines().count();
                                println!("Flatpak-Update-Check: {}", count);
                                if count > 0 {
                                    is_update_available = true;
                                }
                            }
                            Err(err) => {
                                Notification::new()
                                    .summary(&fl!("flatpak-notification-header"))
                                    .body(&err.to_string())
                                    .timeout(Timeout::Milliseconds(5000))
                                    .show_async()
                                    .await
                                    .unwrap();
                            }
                        }
                        if is_update_available {
                            let _ = channel.send(Message::UpdateHasUpdates(true)).await;
                        }

                        sleep(Duration::from_mins(15)).await;
                    }
                })
            }),
            // Watch for application configuration changes.
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| {
                    // for why in update.errors {
                    //     tracing::error!(?why, "app config error");
                    // }

                    Message::UpdateConfig(update.config)
                }),
        ])
    }

    /// Handles messages emitted by the application and its widgets.
    ///
    /// Tasks may be returned for asynchronous execution of code in the background
    /// on the application's async runtime. The application will not exit until all
    /// tasks are finished.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::UpdateConfig(config) => {
                self.config = config;
            }
            Message::UpdateHasUpdates(state) => {
                println!("HasUpdate: {:?}", state);
                self.has_updates = state;
            }
            Message::UpdateIsUpdating(state) => {
                println!("IsUpdating: {:?}", state);
                self.is_updating = state;
            }
            Message::ButtonPressed => {
                self.is_updating = true;
                return Task::run(cosmic::iced::stream::channel(4, Self::update_system), |x| x);
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}

impl AppModel {
    async fn update_system(mut channel: Sender<Action<Message>>) {
        let mut has_succeded = true;
        match Command::new("pkexec")
            .args(["dnf", "upgrade"])
            .arg("-y")
            .arg("--refresh")
            .output()
            .await
        {
            Ok(result) => {
                let result_text = if result.status.success() {
                    fl!("dnf-notification-success")
                } else {
                    has_succeded = false;
                    fl!("dnf-notification-fail")
                };

                Notification::new()
                    .summary(&fl!("dnf-notification-header"))
                    .body(&result_text)
                    .timeout(Timeout::Milliseconds(5000))
                    .show_async()
                    .await
                    .unwrap();
            }
            Err(err) => {
                Notification::new()
                    .summary(&fl!("dnf-notification-header"))
                    .body(&err.to_string())
                    .timeout(Timeout::Milliseconds(5000))
                    .show_async()
                    .await
                    .unwrap();
            }
        }

        match Command::new("flatpak")
            .arg("update")
            .arg("-y")
            .output()
            .await
        {
            Ok(result) => {
                let result_text = if result.status.success() {
                    fl!("flatpak-notification-success")
                } else {
                    has_succeded = false;
                    fl!("flatpak-notification-fail")
                };

                Notification::new()
                    .summary(&fl!("flatpak-notification-header"))
                    .body(&result_text)
                    .timeout(Timeout::Milliseconds(5000))
                    .show_async()
                    .await
                    .unwrap();
            }
            Err(err) => {
                Notification::new()
                    .summary(&fl!("flatpak-notification-header"))
                    .body(&err.to_string())
                    .timeout(Timeout::Milliseconds(5000))
                    .show_async()
                    .await
                    .unwrap();
            }
        }

        if has_succeded {
            let _ = channel
                .send(Action::App(Message::UpdateIsUpdating(false)))
                .await;
            let _ = channel
                .send(Action::App(Message::UpdateHasUpdates(false)))
                .await;
        } else {
            let _ = channel
                .send(Action::App(Message::UpdateIsUpdating(false)))
                .await;
            let _ = channel
                .send(Action::App(Message::UpdateHasUpdates(true)))
                .await;
        }
    }
}
