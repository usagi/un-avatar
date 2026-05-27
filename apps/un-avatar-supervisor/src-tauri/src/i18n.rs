//! UN Avatar Supervisor: `locales/*.toml` と共有 crate [`un_i18n`] の薄い接続層。
//!
//! 設計の権威ソース: <https://github.com/usagi/un-common> の `crates/un-i18n`。

use std::sync::LazyLock;

pub use un_i18n::{apply_locale, SvelteI18nBundle, UnI18nStore};

/// プロセス共有 store。`rust_i18n::i18n!` の backend と Tauri command が同じ flatten 済
/// メッセージを参照する (`(*UN_I18N_STORE).clone()` で rust_i18n に渡す)。
pub static UN_I18N_STORE: LazyLock<UnI18nStore> = LazyLock::new(|| {
	let mut store = UnI18nStore::new();
	store.add_locale_toml("ja-JP", include_str!("../locales/ja-JP.toml"));
	store.add_locale_toml("en-US", include_str!("../locales/en-US.toml"));
	store
});

/// OS locale → サポート一覧の language 一致 → 最終 **`ja-JP`**（作者の第 1 言語）。
pub fn resolve_default_locale(store: &UnI18nStore) -> String {
	un_i18n::resolve_default_locale(store, "ja-JP")
}

#[tauri::command]
pub fn i18n_get_svelte_bundle(locale: String) -> Result<SvelteI18nBundle, String> {
	UN_I18N_STORE
		.svelte_bundle(&locale)
		.ok_or_else(|| format!("i18n: locale '{locale}' is not loaded"))
}

#[tauri::command]
pub fn i18n_available_locales() -> Vec<String> {
	UN_I18N_STORE.available_locales()
}

#[tauri::command]
pub fn i18n_resolve_default_locale() -> String {
	resolve_default_locale(&UN_I18N_STORE)
}
