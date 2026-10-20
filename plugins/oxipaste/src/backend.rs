use std::{future::Future, sync::Arc, time::Duration};

use iced::futures::{
    future::{Either, select},
    pin_mut,
};

pub const MAX_ITEM_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Text,
    Addresses,
    Images,
}
impl Filter {
    pub const ALL: [Self; 4] = [Self::All, Self::Text, Self::Addresses, Self::Images];
    pub fn includes(self, item: &Item) -> bool {
        let address = item.is_text()
            && std::str::from_utf8(&item.bytes).is_ok_and(|text| {
                let text = text.trim();
                !text.contains('\n')
                    && ["https://", "http://", "file://", "/", "./", "../"]
                        .iter()
                        .any(|prefix| text.starts_with(prefix))
            });
        match self {
            Self::All => true,
            Self::Text => item.is_text() && !address,
            Self::Addresses => address,
            Self::Images => item.mime.starts_with("image/"),
        }
    }
}
impl std::fmt::Display for Filter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::All => "All",
            Self::Text => "Text",
            Self::Addresses => "Addresses",
            Self::Images => "Images",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub bytes: Vec<u8>,
    pub mime: String,
}

impl Item {
    pub fn is_text(&self) -> bool {
        self.mime.starts_with("text/")
            || matches!(self.mime.as_str(), "UTF8_STRING" | "STRING" | "TEXT")
    }

    pub fn preview(&self) -> String {
        if !self.is_text() {
            return format!("{} · {} bytes", self.mime, self.bytes.len());
        }
        let text = String::from_utf8_lossy(&self.bytes[..self.bytes.len().min(4096)]);
        let preview: String = text
            .lines()
            .take(3)
            .collect::<Vec<_>>()
            .join("\n")
            .chars()
            .take(240)
            .collect();
        if preview.is_empty() {
            "Empty text".into()
        } else if preview.len() < self.bytes.len() {
            format!("{preview}…")
        } else {
            preview
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        self.mime.to_lowercase().contains(query)
            || (self.is_text()
                && String::from_utf8_lossy(&self.bytes)
                    .to_lowercase()
                    .contains(query))
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    Copy(Arc<Item>),
    Delete(Arc<Item>),
    Clear,
}

#[zbus::proxy(
    interface = "org.Xetibo.OxiPasteDaemon",
    default_service = "org.Xetibo.OxiPasteDaemon",
    default_path = "/org/Xetibo/OxiPasteDaemon"
)]
trait History {
    #[zbus(name = "GetAll")]
    fn get_all(&self) -> zbus::Result<Vec<(Vec<u8>, String)>>;
    #[zbus(name = "CopyContent")]
    fn copy_content(&self, content: &[u8], mime_type: &str) -> zbus::Result<()>;
    #[zbus(name = "DeleteContent")]
    fn delete_content(&self, content: &[u8], mime_type: &str) -> zbus::Result<()>;
    #[zbus(name = "DeleteAll")]
    fn delete_all(&self) -> zbus::Result<()>;
}

async fn bounded<T>(operation: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    let timeout = async_io::Timer::after(Duration::from_secs(8));
    pin_mut!(operation, timeout);
    match select(operation, timeout).await {
        Either::Left((result, _)) => result,
        Either::Right(_) => Err("OxiPaste clipboard backend timed out.".into()),
    }
}

fn error(error: zbus::Error) -> String {
    format!("OxiPaste clipboard backend: {error}")
}

pub async fn read() -> Result<Vec<Arc<Item>>, String> {
    bounded(async {
        let connection = zbus::Connection::session().await.map_err(error)?;
        let proxy = HistoryProxy::new(&connection).await.map_err(error)?;
        let items = proxy.get_all().await.map_err(error)?;
        let mut total = 0usize;
        if items.len() > 5000 {
            return Err("OxiPaste history has too many items".into());
        }
        items
            .into_iter()
            .map(|(bytes, mime)| {
                total = total.saturating_add(bytes.len()).saturating_add(mime.len());
                if bytes.len() > MAX_ITEM_BYTES
                    || total > MAX_HISTORY_BYTES
                    || mime.is_empty()
                    || mime.len() > 256
                    || mime.contains(['\0', '\n', '\r'])
                {
                    return Err("OxiPaste history exceeds supported limits".into());
                }
                Ok(Arc::new(Item { bytes, mime }))
            })
            .collect()
    })
    .await
}

pub async fn act(action: Action) -> Result<(), String> {
    bounded(async {
        let connection = zbus::Connection::session().await.map_err(error)?;
        let proxy = HistoryProxy::new(&connection).await.map_err(error)?;
        match action {
            Action::Copy(item) => proxy.copy_content(&item.bytes, &item.mime).await,
            Action::Delete(item) => proxy.delete_content(&item.bytes, &item.mime).await,
            Action::Clear => proxy.delete_all().await,
        }
        .map_err(error)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_filters_intersect_search_without_treating_multiline_text_as_an_address() {
        let items = [
            Item {
                bytes: b"Project Notes".to_vec(),
                mime: "text/plain".into(),
            },
            Item {
                bytes: b"https://example.org/notes".to_vec(),
                mime: "text/plain".into(),
            },
            Item {
                bytes: b"/tmp/notes.png".to_vec(),
                mime: "text/uri-list".into(),
            },
            Item {
                bytes: b"/tmp/notes\nmore notes".to_vec(),
                mime: "text/plain".into(),
            },
            Item {
                bytes: vec![0, 255, 10],
                mime: "image/png".into(),
            },
        ];
        let matching = |filter: Filter, query| {
            items
                .iter()
                .filter(|item| filter.includes(item) && item.matches(query))
                .count()
        };
        assert_eq!(matching(Filter::All, "notes"), 4);
        assert_eq!(matching(Filter::Text, "notes"), 2);
        assert_eq!(matching(Filter::Addresses, "notes"), 2);
        assert_eq!(matching(Filter::Images, "image"), 1);
        assert_eq!(matching(Filter::Images, "notes"), 0);
        assert_eq!(items[4].bytes, [0, 255, 10]);
    }

    #[test]
    fn lossy_bounded_preview_keeps_original_bytes_and_unicode_search() {
        let item = Item {
            bytes: b"hello\xff\nworld\n".to_vec(),
            mime: "text/plain;charset=utf-8".into(),
        };
        assert!(item.preview().contains("hello"));
        assert!(item.matches("world"));
        assert_eq!(item.bytes, b"hello\xff\nworld\n");
        let unicode = Item {
            bytes: "🐾".repeat(5000).into_bytes(),
            mime: "text/plain".into(),
        };
        assert!(unicode.preview().chars().count() <= 241);
    }

    type CopiedItem = (Vec<u8>, String);

    struct Fixture {
        copied: Arc<std::sync::Mutex<Option<CopiedItem>>>,
        fail: Arc<std::sync::atomic::AtomicBool>,
    }

    #[zbus::interface(name = "org.Xetibo.OxiPasteDaemon")]
    impl Fixture {
        #[zbus(name = "GetAll")]
        fn get_all(&self) -> Vec<(Vec<u8>, String)> {
            vec![
                (b"newer".to_vec(), "text/plain".into()),
                (
                    b"selected\0\xff\n".to_vec(),
                    "text/plain;charset=utf-8".into(),
                ),
            ]
        }

        #[zbus(name = "CopyContent")]
        async fn copy_content(&self, content: Vec<u8>, mime_type: String) -> zbus::fdo::Result<()> {
            async_io::Timer::after(Duration::from_millis(150)).await;
            if self.fail.load(std::sync::atomic::Ordering::Acquire) {
                return Err(zbus::fdo::Error::Failed(
                    "fixture clipboard write failed".into(),
                ));
            }
            *self.copied.lock().unwrap() = Some((content, mime_type));
            Ok(())
        }
    }

    #[test]
    #[ignore = "requires the owned coordinator's private session bus"]
    fn private_bus_copy_waits_for_completion_and_preserves_failed_actions() {
        assert_eq!(std::env::var("OXIBAR_TEST_PRIVATE_BUS").as_deref(), Ok("1"));
        iced::futures::executor::block_on(async {
            let copied = Arc::new(std::sync::Mutex::new(None));
            let fail = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let _server = zbus::connection::Builder::session()
                .unwrap()
                .name("org.Xetibo.OxiPasteDaemon")
                .unwrap()
                .serve_at(
                    "/org/Xetibo/OxiPasteDaemon",
                    Fixture {
                        copied: copied.clone(),
                        fail: fail.clone(),
                    },
                )
                .unwrap()
                .build()
                .await
                .unwrap();
            let items = read().await.unwrap();
            let selected = items[1].clone();
            let operation = act(Action::Copy(selected.clone()));
            pin_mut!(operation);
            let timer = async_io::Timer::after(Duration::from_millis(60));
            pin_mut!(timer);
            let pending = match select(operation, timer).await {
                Either::Right((_, pending)) => pending,
                Either::Left(_) => {
                    panic!("Copy returned before the service acknowledged its write")
                }
            };
            assert!(copied.lock().unwrap().is_none());
            pending.await.unwrap();
            assert_eq!(
                *copied.lock().unwrap(),
                Some((selected.bytes.clone(), selected.mime.clone()))
            );
            fail.store(true, std::sync::atomic::Ordering::Release);
            assert!(
                act(Action::Copy(items[0].clone()))
                    .await
                    .unwrap_err()
                    .contains("fixture clipboard write failed")
            );
            assert_eq!(
                *copied.lock().unwrap(),
                Some((selected.bytes.clone(), selected.mime.clone()))
            );
        });
    }
}
