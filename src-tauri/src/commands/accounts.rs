use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;

use crate::auth::microsoft::{minecraft_chain, poll_device_token, request_device_code, DeviceCode};
use crate::auth::secrets::{minecraft_key, refresh_key};
use crate::auth::skin::{self, SkinData};
use crate::auth::{ensure_token, finish_login, Account, AccountKind, AccountsFile, MsaContext};
use crate::error::{AppError, AppResult};
use crate::events;
use crate::state::{AppState, PendingLogin};

fn changed(app: &AppHandle) {
    let _ = app.emit(events::ACCOUNTS_CHANGED, ());
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> AppResult<AccountsFile> {
    state.accounts.load()
}

#[tauri::command]
pub fn add_offline_account(app: AppHandle, state: State<'_, AppState>, username: String) -> AppResult<Account> {
    let acc = state.accounts.add_offline(&username)?;
    changed(&app);
    Ok(acc)
}

#[tauri::command]
pub fn remove_account(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    let acc = state.accounts.get(&id)?;
    if acc.kind == AccountKind::Microsoft {
        // Сначала токены — чтобы не осталось «осиротевших» секретов.
        state.secrets.delete(&refresh_key(&id))?;
        state.secrets.delete(&minecraft_key(&id))?;
    }
    state.accounts.remove(&id)?;
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn set_default_account(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.accounts.set_default(&id)?;
    changed(&app);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MsaStatus {
    /// Задан ли Client ID (в настройках или при сборке).
    pub configured: bool,
    /// Client ID вшит при сборке.
    pub built_in: bool,
}

#[tauri::command]
pub async fn msa_status(state: State<'_, AppState>) -> AppResult<MsaStatus> {
    Ok(MsaStatus {
        configured: state.msa_client_id().await.is_ok(),
        built_in: crate::auth::microsoft::BUILT_IN_CLIENT_ID.is_some(),
    })
}

/// Шаг 1: получить код, который пользователь вводит на странице Microsoft.
#[tauri::command]
pub async fn msa_begin(state: State<'_, AppState>) -> AppResult<DeviceCode> {
    let client_id = state.msa_client_id().await?;
    let device = request_device_code(&state.client().await, &state.msa_endpoints, &client_id).await?;
    let old = state.pending_login.lock().unwrap().replace(PendingLogin {
        device: device.clone(),
        client_id,
        cancel: CancellationToken::new(),
    });
    if let Some(old) = old {
        old.cancel.cancel();
    }
    Ok(device)
}

/// Шаг 2: дождаться ввода кода и пройти цепочку Xbox → Minecraft.
/// Этапы уходят событием `accounts://login-stage`.
#[tauri::command]
pub async fn msa_finish(app: AppHandle, state: State<'_, AppState>) -> AppResult<Account> {
    let (device, client_id, cancel) = {
        let p = state.pending_login.lock().unwrap();
        let p = p.as_ref().ok_or_else(|| AppError::InvalidInput("вход не начат".into()))?;
        (p.device.clone(), p.client_id.clone(), p.cancel.clone())
    };
    let client = state.client().await;
    let ep = &state.msa_endpoints;
    let emit = |s: &str| {
        let _ = app.emit(events::LOGIN_STAGE, s);
    };
    let result = async {
        emit("waiting");
        let tokens = poll_device_token(&client, ep, &client_id, &device, &cancel).await?;
        let (mc, prof) = tokio::select! {
            r = minecraft_chain(&client, ep, &tokens.access_token, &emit) => r?,
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
        };
        finish_login(state.secrets.as_ref(), &state.accounts, &tokens, &mc, &prof)
    }
    .await;
    state.pending_login.lock().unwrap().take();
    let acc = result?;
    tracing::info!("вход через Microsoft: {}", acc.username);
    changed(&app);
    Ok(acc)
}

#[tauri::command]
pub fn msa_cancel(state: State<'_, AppState>) {
    if let Some(p) = state.pending_login.lock().unwrap().take() {
        p.cancel.cancel();
    }
}

/// Принудительно обновляет токены и профиль (ник, скин) Microsoft-аккаунта.
#[tauri::command]
pub async fn refresh_account(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<Account> {
    let acc = state.accounts.get(&id)?;
    if acc.kind != AccountKind::Microsoft {
        return Ok(acc);
    }
    let client_id = state.msa_client_id().await?;
    let client = state.client().await;
    let ctx = MsaContext { client: &client, ep: &state.msa_endpoints, client_id: &client_id, secrets: state.secrets.as_ref() };
    let (_, updated) = ensure_token(&ctx, &state.accounts, &acc, true).await?;
    changed(&app);
    Ok(updated)
}

#[tauri::command]
pub async fn account_skin(state: State<'_, AppState>, id: String) -> AppResult<Option<SkinData>> {
    let acc = state.accounts.get(&id)?;
    skin::load(&state.client().await, &state.paths.cache_dir().join("skins"), &acc).await
}
