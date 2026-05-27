<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { AppSettings } from "./appSettings";
  import type { AppSettingValue } from "./appTypes";
  import SettingsOptionalNumberField from "./SettingsOptionalNumberField.svelte";

  export let appSettings: AppSettings;
  export let onSetAppSetting: (
    key: keyof AppSettings,
    value: AppSettingValue,
  ) => void;

</script>

<section class="settings-card" data-hint={$_("settings.hints.console_window")}>
  <header class="settings-card-header">
    <h2>{$_("settings.console_window.title")}</h2>
  </header>
  <div class="settings-console-fields">
    <SettingsOptionalNumberField
      label={$_("settings.console_window.x_outer")}
      hint={$_("settings.hints.console_window_x")}
      value={appSettings.console_window_x}
      placeholder={$_("settings.console_window.placeholder_default")}
      onChange={(value) => onSetAppSetting("console_window_x", value)}
    />
    <SettingsOptionalNumberField
      label={$_("settings.console_window.y_outer")}
      hint={$_("settings.hints.console_window_y")}
      value={appSettings.console_window_y}
      placeholder={$_("settings.console_window.placeholder_default")}
      onChange={(value) => onSetAppSetting("console_window_y", value)}
    />
    <SettingsOptionalNumberField
      label={$_("settings.console_window.width_inner")}
      hint={$_("settings.hints.console_window_width")}
      value={appSettings.console_window_width}
      min={0}
      placeholder="1190"
      onChange={(value) => onSetAppSetting("console_window_width", value)}
    />
    <SettingsOptionalNumberField
      label={$_("settings.console_window.height_inner")}
      hint={$_("settings.hints.console_window_height")}
      value={appSettings.console_window_height}
      min={0}
      placeholder="620"
      onChange={(value) => onSetAppSetting("console_window_height", value)}
    />
  </div>
</section>
