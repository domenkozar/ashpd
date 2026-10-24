use std::{collections::HashMap, sync::Arc};

use ashpd::{
    PortalError,
    backend::settings::{SettingsImpl, SettingsSignalEmitter},
    desktop::settings::{APPEARANCE_NAMESPACE, Namespace},
};
use async_trait::async_trait;
use zbus::{OwnedVariant, Variant};

pub const NAME: &str = "org.freedesktop.impl.portal.desktop.ashpd_test";
pub const PATH: &str = "/org/freedesktop/portal/desktop";

pub struct Fixture;

#[async_trait]
impl SettingsImpl for Fixture {
    async fn read(&self, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
        assert_eq!(namespace, APPEARANCE_NAMESPACE);
        match key {
            "color-scheme" => Ok(OwnedVariant::from(1_u32)),
            // Older portal implementations returned an extra variant wrapper.
            "nested" => Ok(Variant::Variant(Box::new(Variant::from(2_u32)))
                .try_into()
                .unwrap()),
            _ => Err(PortalError::InvalidArgument("unknown setting".into())),
        }
    }

    async fn read_all(
        &self,
        namespaces: Vec<String>,
    ) -> Result<HashMap<String, Namespace>, PortalError> {
        assert_eq!(namespaces, [APPEARANCE_NAMESPACE]);
        Ok(HashMap::from([(
            APPEARANCE_NAMESPACE.into(),
            HashMap::from([("color-scheme".into(), OwnedVariant::from(1_u32))]),
        )]))
    }

    fn set_signal_emitter(&mut self, _signal_emitter: Arc<dyn SettingsSignalEmitter>) {}
}

#[cfg(any(feature = "file_chooser", feature = "screencast"))]
#[async_trait]
impl ashpd::backend::request::RequestImpl for Fixture {
    async fn close(&self, _token: ashpd::desktop::HandleToken) {
        panic!("unexpected request cancellation");
    }
}

#[cfg(feature = "file_chooser")]
mod file_chooser {
    use ashpd::{
        MaybeAppID, WindowIdentifierType,
        backend::{Result, file_chooser::FileChooserImpl},
        desktop::{
            HandleToken,
            file_chooser::{OpenFileOptions, SaveFileOptions, SaveFilesOptions, SelectedFiles},
        },
    };

    use async_trait::async_trait;

    use super::Fixture;

    #[async_trait]
    impl FileChooserImpl for Fixture {
        async fn open_file(
            &self,
            _token: HandleToken,
            app_id: Option<MaybeAppID>,
            window_identifier: Option<WindowIdentifierType>,
            title: &str,
            options: OpenFileOptions,
        ) -> Result<SelectedFiles> {
            assert!(app_id.is_none());
            assert!(window_identifier.is_none());
            assert_eq!(title, "Choose a fixture");
            assert_eq!(options.accept_label(), Some("Open fixture"));
            Ok(SelectedFiles::default().uri("file:///tmp/fixture.txt".parse().unwrap()))
        }

        async fn save_file(
            &self,
            _token: HandleToken,
            _app_id: Option<MaybeAppID>,
            _window_identifier: Option<WindowIdentifierType>,
            _title: &str,
            _options: SaveFileOptions,
        ) -> Result<SelectedFiles> {
            panic!("unexpected SaveFile call");
        }

        async fn save_files(
            &self,
            _token: HandleToken,
            _app_id: Option<MaybeAppID>,
            _window_identifier: Option<WindowIdentifierType>,
            _title: &str,
            _options: SaveFilesOptions,
        ) -> Result<SelectedFiles> {
            panic!("unexpected SaveFiles call");
        }
    }
}

#[cfg(feature = "screencast")]
mod screencast {
    use ashpd::{
        MaybeAppID, WindowIdentifierType,
        backend::{
            Result,
            screencast::{ScreencastImpl, SelectSourcesResponse},
            session::{CreateSessionResponse, SessionImpl},
        },
        desktop::{
            CreateSessionOptions, HandleToken,
            screencast::{CursorMode, SelectSourcesOptions, SourceType, StartCastOptions, Streams},
        },
    };
    use enumflags2::BitFlags;

    use async_trait::async_trait;

    use super::Fixture;

    #[async_trait]
    impl SessionImpl for Fixture {
        async fn session_closed(&self, _token: HandleToken) -> Result<()> {
            panic!("unexpected session close");
        }
    }

    #[async_trait]
    impl ScreencastImpl for Fixture {
        fn available_source_types(&self) -> BitFlags<SourceType> {
            SourceType::Monitor | SourceType::Window
        }

        fn available_cursor_mode(&self) -> BitFlags<CursorMode> {
            CursorMode::Hidden | CursorMode::Embedded | CursorMode::Metadata
        }

        async fn create_session(
            &self,
            _token: HandleToken,
            _session_token: HandleToken,
            _app_id: Option<MaybeAppID>,
            _options: CreateSessionOptions,
        ) -> Result<CreateSessionResponse> {
            panic!("unexpected CreateSession call");
        }

        async fn select_sources(
            &self,
            _token: HandleToken,
            _session_token: HandleToken,
            _app_id: Option<MaybeAppID>,
            _options: SelectSourcesOptions,
        ) -> Result<SelectSourcesResponse> {
            panic!("unexpected SelectSources call");
        }

        async fn start_cast(
            &self,
            _token: HandleToken,
            _session_token: HandleToken,
            _app_id: Option<MaybeAppID>,
            _window_identifier: Option<WindowIdentifierType>,
            _options: StartCastOptions,
        ) -> Result<Streams> {
            panic!("unexpected Start call");
        }
    }
}

#[cfg(not(feature = "tokio"))]
pub struct Spawner;

#[cfg(not(feature = "tokio"))]
impl futures_util::task::Spawn for Spawner {
    fn spawn_obj(
        &self,
        future: futures_util::task::FutureObj<'static, ()>,
    ) -> Result<(), futures_util::task::SpawnError> {
        tokio::spawn(future);
        Ok(())
    }
}
