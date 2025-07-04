use std::{mem, path::PathBuf, sync::Arc, time::Duration};

use iced::{
    Background, Length, Task,
    alignment::Vertical,
    task::sipper,
    widget::{button, column, container, row, scrollable, text, text_input},
};
use rfd::{AsyncFileDialog, FileHandle};
use tokio::{fs, time::Instant};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub enum Message {
    SelectFolder,
    SelectedFolder(Option<Arc<FileHandle>>),
    AbortScan,
    ScanComplete,
    Error(String),
    LimitChanged(String),
    StartScan,
    ScanUpdate {
        now_scanned: u64,
        new_paths_over_limit: Vec<OverLimit>,
    },
    ExportCsv,
    CsvExportComplete(Result<String, String>),
}

pub struct UI {
    selecting: bool,
    selected: Option<PathBuf>,
    cancellation_token: Option<CancellationToken>,
    paths_over_limit: Vec<OverLimit>,
    scanned: u64,
    limit_input: String,
    limit: usize,
    scan_limit: usize,
    errors: Vec<String>,
    exporting: bool,
    export_message: Option<String>,
    export_success: bool,
}

#[derive(Debug, Clone)]
pub struct OverLimit {
    path: String,
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
                limit_input: "240".to_string(),
                limit: 240,
                scan_limit: 240,
                errors: Vec::new(),
                exporting: false,
                export_message: None,
                export_success: false,
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
            Message::ScanComplete => {
                if let Some(token) = self.cancellation_token.take() {
                    token.cancel();
                }
                Task::none()
            }
            Message::Error(err) => {
                self.errors.push(err);
                Task::none()
            }
            Message::LimitChanged(limit) => {
                self.limit_input = limit.clone();
                if let Ok(parsed) = limit.parse::<usize>() {
                    self.limit = parsed;
                }
                Task::none()
            }
            Message::StartScan => {
                if let Some(ref folder) = self.selected {
                    self.paths_over_limit.clear();
                    self.errors.clear();
                    self.scanned = 0;
                    let token = CancellationToken::new();
                    self.cancellation_token = Some(token.clone());
                    self.scan_limit = self.limit;
                    self.start_scan(folder.clone(), self.limit, token)
                } else {
                    Task::none()
                }
            }
            Message::ScanUpdate {
                now_scanned,
                new_paths_over_limit,
            } => {
                self.scanned = now_scanned;
                self.paths_over_limit.extend(new_paths_over_limit);
                Task::none()
            }
            Message::ExportCsv => {
                if self.paths_over_limit.is_empty() {
                    Task::none()
                } else {
                    self.exporting = true;
                    self.export_message = None;
                    let paths_to_export = self.paths_over_limit.clone();
                    Task::future(async move {
                        let file_handle = AsyncFileDialog::new()
                            .set_file_name("path_length_report.csv")
                            .add_filter("CSV", &["csv"])
                            .save_file()
                            .await;

                        if let Some(file_handle) = file_handle {
                            let mut csv_content = String::from("Path,Length\n");
                            let export_count = paths_to_export.len();
                            for path in &paths_to_export {
                                csv_content.push_str(&format!(
                                    "\"{}\",{}\n",
                                    path.path.replace("\"", "\"\""),
                                    path.size
                                ));
                            }

                            match tokio::fs::write(file_handle.path(), csv_content).await {
                                Ok(_) => Message::CsvExportComplete(Ok(format!(
                                    "Exported {} paths to {}",
                                    export_count,
                                    file_handle.path().display()
                                ))),
                                Err(e) => Message::CsvExportComplete(Err(format!(
                                    "Failed to write CSV file: {}",
                                    e
                                ))),
                            }
                        } else {
                            Message::CsvExportComplete(Err("Export cancelled".to_string()))
                        }
                    })
                }
            }
            Message::CsvExportComplete(result) => {
                self.exporting = false;
                match result {
                    Ok(success_msg) => {
                        self.export_message = Some(success_msg);
                        self.export_success = true;
                        Task::none()
                    }
                    Err(error_msg) => {
                        self.export_message = Some(error_msg);
                        self.export_success = false;
                        Task::none()
                    }
                }
            }
        }
    }

    pub fn view(&self) -> iced::Element<Message> {
        let main_controls = column![
            button(text("Select Folder")).on_press_maybe(if self.selecting {
                None
            } else {
                Some(Message::SelectFolder)
            }),
            row![
                text("Path Length Limit:"),
                text_input("", &self.limit_input)
                    .on_input(Message::LimitChanged)
                    .on_submit(Message::StartScan)
                    .width(Length::Fixed(100.0)),
            ]
            .spacing(10)
            .align_y(Vertical::Center),
            row![
                button(text("Start Scan")).on_press_maybe(
                    if self.selected.is_some() && !self.cancellation_token.is_some() {
                        Some(Message::StartScan)
                    } else {
                        None
                    }
                ),
                button(text("Abort")).on_press_maybe(if self.cancellation_token.is_some() {
                    Some(Message::AbortScan)
                } else {
                    None
                }),
                button(text("Export CSV")).on_press_maybe(
                    if !self.paths_over_limit.is_empty()
                        && !self.exporting
                        && self.cancellation_token.is_none()
                    {
                        Some(Message::ExportCsv)
                    } else {
                        None
                    }
                ),
            ]
            .spacing(10),
        ]
        .spacing(10);

        let mut content = column![main_controls].spacing(20);

        if let Some(selected) = &self.selected {
            content = content.push(text(format!("Selected: {}", selected.to_string_lossy())));
        }

        if self.cancellation_token.is_some() {
            content =
                content.push(text(format!("Scanning... {} paths checked", self.scanned)).size(16));
        }

        if !self.paths_over_limit.is_empty() {
            let results_title = text(format!(
                "Found {} paths over limit ({})",
                self.paths_over_limit.len(),
                self.scan_limit
            ))
            .size(18);

            content = content.push(results_title);
        }

        if self.exporting {
            content = content.push(text("Exporting to CSV...").size(16));
        }

        if let Some(ref message) = self.export_message {
            let export_text = if self.export_success {
                text(message)
                    .size(16)
                    .color(iced::Color::from_rgb(0.0, 0.6, 0.0))
            } else {
                text(message)
                    .size(16)
                    .color(iced::Color::from_rgb(0.8, 0.2, 0.2))
            };
            content = content.push(export_text);
        }

        if !self.errors.is_empty() {
            let errors_title = text(format!("Errors ({})", self.errors.len()))
                .size(18)
                .color(iced::Color::from_rgb(0.8, 0.2, 0.2));

            let errors_list = scrollable(
                self.errors
                    .iter()
                    .fold(column![], |col, error| {
                        col.push(container(text(error).size(12)).padding(10).style(|_| {
                            container::Style {
                                background: Some(Background::Color(iced::Color::from_rgb(
                                    1.0, 0.95, 0.95,
                                ))),
                                border: iced::Border {
                                    color: iced::Color::from_rgb(0.8, 0.6, 0.6),
                                    width: 1.0,
                                    radius: 0.0.into(),
                                },
                                ..Default::default()
                            }
                        }))
                    })
                    .spacing(5),
            )
            .height(Length::Fixed(150.0));

            content = content.push(errors_title).push(errors_list);
        }

        content.padding(20).into()
    }

    fn start_scan(
        &mut self,
        root: PathBuf,
        limit: usize,
        token: CancellationToken,
    ) -> Task<Message> {
        let sipper = sipper(move |mut sender| async move {
            let mut stack = vec![root];

            let mut scanned: u64 = 0;
            let mut over_limit: Vec<OverLimit> = Vec::new();
            let mut last_update = Instant::now();

            token
                .run_until_cancelled(async move {
                    while let Some(path) = stack.pop() {
                        match fs::read_dir(&path).await {
                            Ok(mut entries) => {
                                while let Ok(Some(entry)) = entries.next_entry().await {
                                    let entry_path = entry.path();
                                    let path_length = entry_path.as_os_str().len();

                                    if path_length > limit {
                                        over_limit.push(OverLimit {
                                            path: entry_path
                                                .as_os_str()
                                                .to_string_lossy()
                                                .to_string(),
                                            size: path_length as u64,
                                        });
                                    }

                                    match entry.metadata().await {
                                        Ok(metadata) => {
                                            if metadata.is_dir() {
                                                stack.push(entry_path);
                                            }
                                        }
                                        Err(err) => {
                                            sender
                                                .send(Message::Error(format!(
                                                    "Error reading metadata for {}: {}",
                                                    entry_path.display(),
                                                    err
                                                )))
                                                .await;
                                        }
                                    }

                                    scanned += 1;

                                    let now = Instant::now();
                                    if now - last_update > Duration::from_millis(100) {
                                        sender
                                            .send(Message::ScanUpdate {
                                                now_scanned: scanned,
                                                new_paths_over_limit: mem::take(&mut over_limit),
                                            })
                                            .await;
                                        last_update = now;
                                    }
                                }
                            }
                            Err(err) => {
                                sender
                                    .send(Message::Error(format!(
                                        "Error reading directory {}: {}",
                                        path.display(),
                                        err
                                    )))
                                    .await;
                            }
                        }
                    }

                    sender
                        .send(Message::ScanUpdate {
                            now_scanned: scanned,
                            new_paths_over_limit: mem::take(&mut over_limit),
                        })
                        .await;
                })
                .await;
        });

        Task::sip(sipper, |value| value, |_| Message::ScanComplete)
    }
}
