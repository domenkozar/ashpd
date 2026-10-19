#![cfg(feature = "settings")]

use std::collections::HashMap;

use ashpd::desktop::settings::{APPEARANCE_NAMESPACE, ColorScheme, Settings};
use futures_util::StreamExt;
use zbus::{Connection, OwnedVariant, Variant};

struct SettingsPortal;

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl SettingsPortal {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }

    fn read(&self, namespace: &str, key: &str) -> zbus::fdo::Result<OwnedVariant> {
        assert_eq!(namespace, APPEARANCE_NAMESPACE);
        match key {
            "color-scheme" => Ok(OwnedVariant::from(1_u32)),
            // Older portal implementations returned an extra variant wrapper.
            "nested" => Ok(Variant::Variant(Box::new(Variant::from(2_u32)))
                .try_into()
                .unwrap()),
            _ => Err(zbus::fdo::Error::InvalidArgs("unknown setting".into())),
        }
    }

    fn read_all(&self, namespaces: Vec<String>) -> HashMap<String, HashMap<String, OwnedVariant>> {
        assert_eq!(namespaces, [APPEARANCE_NAMESPACE]);
        HashMap::from([(
            APPEARANCE_NAMESPACE.into(),
            HashMap::from([("color-scheme".into(), OwnedVariant::from(1_u32))]),
        )])
    }
}

#[cfg(feature = "screencast")]
struct ScreencastPortal;

#[cfg(feature = "screencast")]
#[zbus::interface(name = "org.freedesktop.portal.ScreenCast")]
impl ScreencastPortal {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }
    #[zbus(property)]
    fn available_source_types(&self) -> u32 {
        3
    }
    #[zbus(property)]
    fn available_cursor_modes(&self) -> u32 {
        7
    }
}

#[cfg(feature = "file_chooser")]
struct FileChooserPortal;

#[cfg(feature = "file_chooser")]
#[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
impl FileChooserPortal {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        4
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
        let results = HashMap::from([(
            "uris",
            OwnedVariant::try_from(Variant::from(vec!["file:///tmp/fixture.txt".to_owned()]))?,
        )]);
        // Emit before returning: the request must already be subscribed.
        connection
            .emit_signal(
                Some(sender),
                &path,
                "org.freedesktop.portal.Request",
                "Response",
                &(0_u32, results),
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
        let server = zbus::connection::Builder::session()
            .name("org.freedesktop.portal.Desktop")
            .serve_at("/org/freedesktop/portal/desktop", SettingsPortal)
            .build()
            .await
            .unwrap();
        #[cfg(feature = "screencast")]
        server
            .object_server()
            .at("/org/freedesktop/portal/desktop", ScreencastPortal)
            .await
            .unwrap();
        #[cfg(feature = "file_chooser")]
        server
            .object_server()
            .at("/org/freedesktop/portal/desktop", FileChooserPortal)
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
        server
            .emit_signal(
                None::<&str>,
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
                "SettingChanged",
                &(APPEARANCE_NAMESPACE, "color-scheme", Variant::from(2_u32)),
            )
            .await
            .unwrap();
        let setting = changes.next().await.unwrap();
        assert_eq!(setting.namespace(), APPEARANCE_NAMESPACE);
        assert_eq!(setting.key(), "color-scheme");
        assert_eq!(u32::try_from(setting.value()).unwrap(), 2);
    })
    .await
    .expect("settings portal operations timed out");
}
