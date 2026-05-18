#!/usr/bin/env sh
set -eu

apply=0
if [ "${1:-}" = "--apply" ]; then
  apply=1
fi

if [ "$apply" -eq 0 ]; then
  echo "Dry run. Re-run with --apply after quitting AetherSDR to edit settings."
else
  echo "Applying layout reset. A backup will be written beside each edited file."
fi

config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
settings_paths="
$config_home/AetherSDR/AetherSDR.settings
$HOME/Library/Preferences/AetherSDR/AetherSDR/AetherSDR.settings
$HOME/Library/Preferences/AetherSDR/AetherSDR.settings
"

keys="
Applet_TUN
Applet_AMP
AppletOrder
AppletPanelVisible
AppletPanelFloating
AppletPanelFloatGeometry
FloatingApplet_TUN_IsFloating
FloatingApplet_AMP_IsFloating
FloatingApplet_TUN_Geometry
FloatingApplet_AMP_Geometry
"

found=0
for path in $settings_paths; do
  if [ ! -f "$path" ]; then
    continue
  fi
  found=1
  echo "Settings: $path"
  for key in $keys; do
    if grep -q "<$key>" "$path"; then
      echo "  found <$key>"
    fi
  done

  if [ "$apply" -eq 1 ]; then
    stamp="$(date +%Y%m%d-%H%M%S)"
    cp "$path" "$path.egb-backup-$stamp"
    for key in $keys; do
      perl -0pi -e "s/\\n?\\s*<$key>.*?<\\/$key>//gs" "$path"
    done
    echo "  edited, backup: $path.egb-backup-$stamp"
  fi
done

plist="$HOME/Library/Preferences/com.aethersdr.AetherSDR.plist"
if [ -f "$plist" ]; then
  found=1
  echo "Preference plist exists: $plist"
  echo "  layout-only script leaves this file untouched."
  echo "  use AetherSDR Support reset for a full app-specific reset."
fi

if [ "$found" -eq 0 ]; then
  echo "No AetherSDR settings files found in known locations."
fi
