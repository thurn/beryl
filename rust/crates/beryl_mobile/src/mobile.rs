use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::plugin::mobile::PluginInvokeError;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{Manager, Runtime};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_beryl_mobile);

#[derive(Deserialize)]
pub struct Device {
    pub os: String,
    pub model: String,
    pub simulator: bool,
}

#[derive(Deserialize)]
struct Snapshot {
    path: PathBuf,
}

#[derive(Serialize)]
struct ShareRequest {
    path: PathBuf,
}

pub struct Mobile<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Mobile<R> {
    pub async fn device(&self) -> Result<Device, PluginInvokeError> {
        self.0.run_mobile_plugin_async("device", ()).await
    }

    pub async fn share(&self, path: PathBuf) -> Result<(), PluginInvokeError> {
        self.0
            .run_mobile_plugin_async("share", ShareRequest { path })
            .await
    }

    pub async fn snapshot(&self) -> Result<PathBuf, PluginInvokeError> {
        let reply: Snapshot = self.0.run_mobile_plugin_async("snapshot", ()).await?;
        Ok(reply.path)
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("beryl-mobile")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let handle =
                api.register_android_plugin("ca.thurn.beryl.mobile", "BerylMobilePlugin")?;
            #[cfg(target_os = "ios")]
            let handle = api.register_ios_plugin(init_plugin_beryl_mobile)?;
            app.manage(Mobile(handle));
            Ok(())
        })
        .build()
}
