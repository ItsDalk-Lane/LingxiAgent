# @lingxi/plugin-components

React component primitives for Lingxi plugin WebViews/iframes.

```tsx
import {
  Button,
  CardShell,
  LingxiThemeProvider,
  SettingRow,
  Switch,
} from '@lingxi/plugin-components';
import '@lingxi/plugin-components/styles.css';

export function PluginPanel() {
  return (
    <LingxiThemeProvider mode="inherit">
      <CardShell title="Sync">
        <SettingRow
          label="Enabled"
          hint="Follows the current Lingxi theme."
          control={<Switch checked label="On" />}
        />
        <Button variant="primary">Run</Button>
      </CardShell>
    </LingxiThemeProvider>
  );
}
```

`LingxiThemeProvider` has three modes:

- `inherit`: use host CSS variables when the WebView/iframe receives them, then fall back to Lingxi defaults from `styles.css`.
- `lingxi`: set one of Lingxi's named theme token groups, such as `warm-paper` or `midnight`.
- `custom`: set only the tokens you provide. Missing tokens still fall back through host variables and SDK defaults.

Components intentionally expose stable `hana-plugin-*` classes so plugin authors can add small local refinements without depending on Lingxi renderer internals.
