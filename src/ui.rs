use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use iced::{
    Task,
    task::{Sipper, sipper},
    widget::{button, column, row, text},
};
use rfd::{AsyncFileDialog, FileHandle};
use tokio::fs;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub enum Message {
    SelectFolder,
    SelectedFolder(Option<Arc<FileHandle>>),
    AbortScan,
    ScanComplete,
    FoundOverLimit(OverLimit),
    Error(String),
}

pub struct UI {
    selecting: bool,
    selected: Option<PathBuf>,
    cancellation_token: Option<CancellationToken>,
    paths_over_limit: Vec<OverLimit>,
    scanned: u64,
}

#[derive(Debug, Clone)]
struct OverLimit {
    path: PathBuf,
    size: u64,
}

impl UI {
    pub fn start() -> (Self, Task<Message>) {
        (
            Self {
                selecting: false,
                selected: None,
                cancellation_token: None,
                paths_over_limit: Vec::new(),
                scanned: 0,
            },
            Task::none(),
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SelectFolder => {
                self.selecting = true;
                Task::future(async {
                    let folder = AsyncFileDialog::new().pick_folder().await;
                    Message::SelectedFolder(folder.map(Arc::new))
                })
            }
            Message::SelectedFolder(selected) => {
                self.selecting = false;
                if let Some(selected) = selected {
                    if let Some(selected) = Arc::into_inner(selected) {
                        let selected: PathBuf = selected.path().into();
                        self.selected = Some(selected.clone());
                        // Start Scan here
                    }
                }
                Task::none()
            }
            Message::AbortScan => {
                if let Some(token) = self.cancellation_token.take() {
                    token.cancel();
                }
                Task::none()
            }
            Message::FoundOverLimit(over_limit) => {
                self.paths_over_limit.push(over_limit);
                Task::none()
            }
            Message::ScanComplete => {
                if let Some(token) = self.cancellation_token.take() {
                    token.cancel();
                }

                Task::none()
            }
            Message::Error(err) => {
                panic!("{}", err);
            }
        }
    }

    pub fn view(&self) -> iced::Element<Message> {
        column![row![
            button(text("Select Folder")).on_press_maybe(if self.selecting {
                None
            } else {
                Some(Message::SelectFolder)
            }),
            button(text("Abort")).on_press_maybe(if self.cancellation_token.is_some() {
                Some(Message::AbortScan)
            } else {
                None
            }),
        ],]
        .push_maybe(
            self.selected
                .as_ref()
                .map(|selected| text(selected.to_string_lossy())),
        )
        .padding(20)
        .spacing(20)
        .into()
    }

    fn start_scan(
        &mut self,
        root: PathBuf,
        limit: usize,
        token: CancellationToken,
    ) -> Task<Message> {
        let sipper = sipper(move |mut sender| async move {
            let mut stack = vec![root];

            token.run_until_cancelled(async move {
                while let Some(path) = stack.pop() {
                    match fs::read_dir(path).await {
                        Ok(mut entries) => match entries.next_entry().await {
                            Ok(Some(entry)) => {
                                let path = entry.path();
                                let path_length = path.as_os_str().len();

                                match entry.metadata().await {
                                    Ok(metadata) => {
                                        if metadata.is_dir() {
                                            stack.push(path.clone());
                                        }
                                    }
                                    Err(err) => sender.send(Message::Error(err.to_string())).await,
                                }

                                if path_length > limit {
                                    sender
                                        .send(Message::FoundOverLimit(OverLimit {
                                            path: path,
                                            size: path_length as u64,
                                        }))
                                        .await;
                                } else {
                                    stack.push(entry.path());
                                }
                            }
                            Ok(None) => (),
                            Err(err) => {
                                sender.send(Message::Error(err.to_string())).await;
                            }
                        },
                        Err(err) => {
                            sender.send(Message::Error(err.to_string())).await;
                        }
                    }
                }
            });
        });

        Task::sip(sipper, |value| value, |value| Message::ScanComplete)
    }
}
