use std::sync::Arc;

use iced::{
    Task,
    widget::{button, column, text},
};
use rfd::{AsyncFileDialog, FileHandle};

#[derive(Debug, Clone)]
pub enum Message {
    SelectFolder,
    SelectedFolder(Option<Arc<FileHandle>>),
}

pub struct UI {
    selecting: bool,
    selected: Option<String>,
}

impl UI {
    pub fn start() -> (Self, Task<Message>) {
        (
            Self {
                selecting: false,
                selected: None,
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
                        self.selected = Some(selected.path().to_string_lossy().to_string());
                    }
                }
                Task::none()
            }
        }
    }

    pub fn view(&self) -> iced::Element<Message> {
        column![
            text("Hello World!"),
            button(text("Select Folder")).on_press_maybe(if self.selecting {
                None
            } else {
                Some(Message::SelectFolder)
            }),
        ]
        .push_maybe(self.selected.as_ref().map(|selected| text(selected)))
        .padding(20)
        .spacing(20)
        .into()
    }
}
