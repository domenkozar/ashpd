#![cfg(all(feature = "backend", feature = "settings"))]

#[path = "portal_wire/backend.rs"]
mod backend;

use std::collections::HashMap;

use ashpd::desktop::settings::{APPEARANCE_NAMESPACE, ColorScheme, Settings};
use futures_util::StreamExt;
use zbus::{Connection, OwnedVariant, Variant};

// The client speaks org.freedesktop.portal.*, while the backend implements
// org.freedesktop.impl.portal.*. Forward only the protocol differences normally
// handled by xdg-desktop-portal; all fixture data comes from ASHPD's backend.
struct SettingsPortal(zbus::Proxy<'static>);

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl SettingsPortal {
    #[zbus(property, name = "version")]
    async fn version(&self) -> zbus::fdo::Result<u32> {
        Ok(self.0.get_property("version").await?)
    }

    async fn read(&self, namespace: &str, key: &str) -> zbus::fdo::Result<OwnedVariant> {
        Ok(self.0.call("Read", &(namespace, key)).await?)
    }

    async fn read_all(
        &self,
        namespaces: Vec<String>,
    ) -> zbus::fdo::Result<HashMap<String, HashMap<String, OwnedVariant>>> {
        Ok(self.0.call("ReadAll", &(namespaces,)).await?)
    }
}

#[cfg(feature = "screencast")]
struct ScreencastPortal(zbus::Proxy<'static>);

#[cfg(feature = "screencast")]
#[zbus::interface(name = "org.freedesktop.portal.ScreenCast")]
impl ScreencastPortal {
    #[zbus(property, name = "version")]
    async fn version(&self) -> zbus::fdo::Result<u32> {
        Ok(self.0.get_property("version").await?)
    }
    #[zbus(property)]
    async fn available_source_types(&self) -> zbus::fdo::Result<u32> {
        Ok(self.0.get_property("AvailableSourceTypes").await?)
    }
    #[zbus(property)]
    async fn available_cursor_modes(&self) -> zbus::fdo::Result<u32> {
        Ok(self.0.get_property("AvailableCursorModes").await?)
    }
}

#[cfg(feature = "file_chooser")]
struct FileChooserPortal(zbus::Proxy<'static>);

#[cfg(feature = "file_chooser")]
#[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
impl FileChooserPortal {
    #[zbus(property, name = "version")]
    async fn version(&self) -> zbus::fdo::Result<u32> {
        Ok(self.0.get_property("version").await?)
    }

    async fn open_file(
        &self,
        parent_window: &str,
        title: &str,
        options: HashMap<String, OwnedVariant>,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<zbus::OwnedObjectPath> {
        assert!(parent_window.is_empty());
        assert_eq!(title, "Choose a fixture");
        let sender = header.sender().unwrap();
        let token = <&str>::try_from(&options["handle_token"])?;
        let path = zbus::OwnedObjectPath::try_from(format!(
            "/org/freedesktop/portal/desktop/request/{}/{token}",
            sender.as_str().trim_start_matches(':').replace('.', "_")
        ))?;
        let (response, results): (u32, HashMap<String, OwnedVariant>) = self
            .0
            .call("OpenFile", &(&path, "", parent_window, title, options))
            .await?;
        // Emit before returning: the request must already be subscribed.
        connection
            .emit_signal(
                Some(sender),
                &path,
                "org.freedesktop.portal.Request",
                "Response",
                &(response, results),
            )
            .await?;
        Ok(path)
    }
}

#[tokio::test]
async fn settings_wire_reads_properties_and_signals() {
    // Never claim the desktop portal name on a developer's real session bus.
    if std::env::var_os("ASHPD_TEST_PRIVATE_DBUS").is_none() {
        return;
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let backend_connection = Connection::session().await.unwrap();
        let builder = ashpd::backend::Builder::new(backend::NAME)
            .unwrap()
            .settings(backend::Fixture);
        #[cfg(feature = "screencast")]
        let builder = builder.screencast(backend::Fixture);
        #[cfg(feature = "file_chooser")]
        let builder = builder.file_chooser(backend::Fixture);
        #[cfg(not(feature = "tokio"))]
        let builder = builder.with_spawn(backend::Spawner);

        let bus = zbus::fdo::DBusProxy::new(&backend_connection)
            .await
            .unwrap();
        let mut owners = bus.receive_name_owner_changed().await.unwrap();
        let task_connection = backend_connection.clone();
        let backend_task = tokio::spawn(async move {
            builder
                .build_with_connection(task_connection)
                .await
                .unwrap();
        });
        // Builder requests its name only after registering every interface.
        while let Some(owner) = owners.next().await {
            if owner.args().unwrap().name().as_str() == backend::NAME {
                break;
            }
        }
        let settings_backend = backend_proxy(&backend_connection, "Settings").await;
        let mut backend_changes = settings_backend
            .receive_signal("SettingChanged")
            .await
            .unwrap();
        let server = zbus::connection::Builder::session()
            .name("org.freedesktop.portal.Desktop")
            .serve_at(backend::PATH, SettingsPortal(settings_backend.clone()))
            .build()
            .await
            .unwrap();
        #[cfg(feature = "screencast")]
        server
            .object_server()
            .at(
                backend::PATH,
                ScreencastPortal(backend_proxy(&backend_connection, "ScreenCast").await),
            )
            .await
            .unwrap();
        #[cfg(feature = "file_chooser")]
        server
            .object_server()
            .at(
                backend::PATH,
                FileChooserPortal(backend_proxy(&backend_connection, "FileChooser").await),
            )
            .await
            .unwrap();
        let client = Connection::session().await.unwrap();
        let settings = Settings::with_connection(client.clone()).await.unwrap();
        assert_eq!(settings.version(), 2);
        assert_eq!(
            settings.color_scheme().await.unwrap(),
            ColorScheme::PreferDark
        );
        assert_eq!(
            settings
                .read::<u32>(APPEARANCE_NAMESPACE, "nested")
                .await
                .unwrap(),
            2
        );
        assert!(
            settings
                .read::<u32>(APPEARANCE_NAMESPACE, "unknown")
                .await
                .is_err()
        );
        let all = settings.read_all(&[APPEARANCE_NAMESPACE]).await.unwrap();
        assert_eq!(
            u32::try_from(&all[APPEARANCE_NAMESPACE]["color-scheme"]).unwrap(),
            1
        );
        #[cfg(feature = "screencast")]
        {
            use ashpd::desktop::screencast::Screencast;
            let screencast = Screencast::with_connection(client.clone()).await.unwrap();
            assert_eq!(screencast.available_source_types().await.unwrap().bits(), 3);
            assert_eq!(screencast.available_cursor_modes().await.unwrap().bits(), 7);
        }
        #[cfg(feature = "file_chooser")]
        {
            use ashpd::desktop::file_chooser::OpenFileRequest;
            let selected = OpenFileRequest::default()
                .title("Choose a fixture")
                .accept_label("Open fixture")
                .connection(Some(client))
                .send()
                .await
                .unwrap()
                .response()
                .unwrap();
            assert_eq!(
                selected
                    .uris()
                    .iter()
                    .map(|uri| uri.as_str())
                    .collect::<Vec<_>>(),
                ["file:///tmp/fixture.txt"]
            );
        }
        let mut changes = settings.receive_setting_changed().await.unwrap();
        backend_connection
            .emit_signal(
                None::<&str>,
                backend::PATH,
                "org.freedesktop.impl.portal.Settings",
                "SettingChanged",
                &(APPEARANCE_NAMESPACE, "color-scheme", Variant::from(2_u32)),
            )
            .await
            .unwrap();
        let message = backend_changes.next().await.unwrap();
        let message_body = message.body();
        let body: (&str, &str, Variant<'_>) = message_body.deserialize().unwrap();
        server
            .emit_signal(
                None::<&str>,
                backend::PATH,
                "org.freedesktop.portal.Settings",
                "SettingChanged",
                &body,
            )
            .await
            .unwrap();
        let setting = changes.next().await.unwrap();
        assert_eq!(setting.namespace(), APPEARANCE_NAMESPACE);
        assert_eq!(setting.key(), "color-scheme");
        assert_eq!(u32::try_from(setting.value()).unwrap(), 2);
        backend_task.abort();
        assert!(backend_task.await.unwrap_err().is_cancelled());
    })
    .await
    .expect("settings portal operations timed out");
}

async fn backend_proxy(connection: &Connection, interface: &str) -> zbus::Proxy<'static> {
    zbus::Proxy::new_owned(
        connection.clone(),
        backend::NAME,
        backend::PATH,
        format!("org.freedesktop.impl.portal.{interface}"),
    )
    .await
    .unwrap()
}
