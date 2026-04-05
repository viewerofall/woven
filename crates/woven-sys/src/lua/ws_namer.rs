//! Workspace auto-namer — replaces numeric workspace labels in the overlay
//! with smart names derived from the windows on each workspace.
//!
//! Two layers:
//!   1. App classification + combo rules (instant, deterministic)
//!   2. Frequency table via woven.store (learns user preferences over time)

use std::collections::HashMap;
use woven_common::types::Workspace;

// ── App categories ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Terminal,
    Editor,
    Browser,
    Chat,
    Media,
    FileManager,
    Design,
    Video,
    Gaming,
    System,
    Documents,
    Mail,
    Torrent,
    Modeling,
    Vm,
    Office,
    Photography,
    Science,
    Unknown,
}

impl Category {
    fn label(self) -> &'static str {
        match self {
            Self::Terminal    => "terminal",
            Self::Editor      => "editor",
            Self::Browser     => "web",
            Self::Chat        => "chat",
            Self::Media       => "media",
            Self::FileManager => "files",
            Self::Design      => "design",
            Self::Video       => "video",
            Self::Gaming      => "gaming",
            Self::System      => "system",
            Self::Documents   => "docs",
            Self::Mail        => "mail",
            Self::Torrent     => "torrent",
            Self::Modeling    => "3d",
            Self::Vm          => "vm",
            Self::Office      => "office",
            Self::Photography => "photo",
            Self::Science     => "science",
            Self::Unknown     => "misc",
        }
    }

    fn plural(self) -> &'static str {
        match self {
            Self::Terminal    => "terminals",
            Self::Editor      => "editors",
            Self::Browser     => "browsers",
            Self::Chat        => "chats",
            Self::Media       => "media",
            Self::FileManager => "files",
            Self::Design      => "design",
            Self::Video       => "video",
            Self::Gaming      => "gaming",
            Self::System      => "system",
            Self::Documents   => "docs",
            Self::Mail        => "mail",
            Self::Torrent     => "torrents",
            Self::Modeling    => "3d",
            Self::Vm          => "vms",
            Self::Office      => "office",
            Self::Photography => "photos",
            Self::Science     => "science",
            Self::Unknown     => "misc",
        }
    }
}

/// Classify a window class string into a category.
fn classify(class: &str) -> Category {
    let c = class.to_lowercase();

    // Terminals
    if matches!(c.as_str(),
        "kitty" | "alacritty" | "foot" | "wezterm" | "warp"
        | "gnome-terminal" | "gnome-terminal-server"
        | "konsole" | "xfce4-terminal" | "tilix" | "terminator"
        | "sakura" | "st" | "st-256color" | "rxvt" | "urxvt"
        | "xterm" | "mate-terminal" | "lxterminal" | "terminology"
        | "cool-retro-term" | "guake" | "yakuake" | "tilda"
        | "rio" | "ghostty" | "contour" | "tabby" | "hyper"
        | "black-box" | "ptyxis"
    ) { return Category::Terminal; }

    // Editors / IDEs
    if matches!(c.as_str(),
        "code" | "code-oss" | "vscodium" | "visual studio code"
        | "zed" | "neovide" | "gvim" | "emacs" | "xemacs"
        | "sublime_text" | "sublime-text" | "gedit" | "kate" | "kwrite"
        | "mousepad" | "pluma" | "xed" | "featherpad" | "leafpad"
        | "gnome-text-editor" | "helix" | "lapce" | "lite-xl"
        | "geany" | "bluefish" | "brackets" | "atom"
    ) { return Category::Editor; }
    // JetBrains family
    if c.contains("intellij") || c.contains("pycharm") || c.contains("webstorm")
        || c.contains("clion") || c.contains("rider") || c.contains("goland")
        || c.contains("phpstorm") || c.contains("rubymine") || c.contains("datagrip")
        || c.contains("android-studio") || c.contains("fleet")
    { return Category::Editor; }
    // Eclipse variants
    if c.contains("eclipse") || c.contains("netbeans") { return Category::Editor; }

    // Browsers
    if matches!(c.as_str(),
        "firefox" | "firefox-esr" | "firefox-developer-edition"
        | "chromium" | "chromium-browser"
        | "google-chrome" | "google-chrome-stable" | "google-chrome-beta"
        | "brave" | "brave-browser" | "brave-browser-stable"
        | "zen" | "zen-alpha" | "zen-beta" | "zen-browser"
        | "vivaldi" | "vivaldi-stable" | "vivaldi-snapshot"
        | "librewolf" | "tor browser" | "torbrowser"
        | "epiphany" | "midori" | "qutebrowser" | "nyxt" | "luakit"
        | "falkon" | "konqueror" | "min" | "otter-browser" | "waterfox"
        | "floorp" | "ungoogled-chromium" | "opera"
    ) { return Category::Browser; }

    // Chat / Social
    if matches!(c.as_str(),
        "discord" | "vesktop" | "webcord" | "armcord" | "equibop"
        | "slack" | "telegram-desktop" | "telegramdesktop" | "64gram"
        | "signal" | "signal-desktop"
        | "element" | "element-desktop" | "schildichat-desktop"
        | "nheko" | "fractal" | "fluffychat"
        | "whatsapp" | "whatsapp-desktop" | "zapzap"
        | "teams" | "microsoft teams" | "teams-for-linux"
        | "revolt" | "guilded" | "mumble" | "teamspeak"
        | "wire" | "session-desktop" | "briar-desktop"
        | "pidgin" | "dino" | "gajim" | "hexchat" | "irssi" | "weechat"
        | "cinny"
    ) { return Category::Chat; }

    // Mail
    if matches!(c.as_str(),
        "thunderbird" | "geary" | "evolution" | "kmail"
        | "mailspring" | "tutanota" | "protonmail-bridge"
        | "betterbird" | "claws-mail" | "sylpheed"
    ) { return Category::Mail; }

    // Media players
    if matches!(c.as_str(),
        "spotify" | "vlc" | "mpv" | "celluloid" | "totem"
        | "audacious" | "clementine" | "strawberry" | "elisa"
        | "rhythmbox" | "lollypop" | "amberol" | "tidal-hifi"
        | "cmus" | "ncmpcpp" | "nuclear" | "museeks" | "plexamp"
        | "plex" | "jellyfin" | "kodi" | "stremio"
        | "haruna" | "smplayer" | "mplayer" | "parole"
        | "gnome-music" | "g4music" | "shortwave" | "pithos"
        | "blanket" | "mousai" | "sonixd" | "feishin"
    ) { return Category::Media; }

    // File managers
    if matches!(c.as_str(),
        "nautilus" | "org.gnome.nautilus" | "thunar" | "dolphin"
        | "nemo" | "pcmanfm" | "pcmanfm-qt" | "caja" | "spacefm"
        | "krusader" | "double commander" | "doublecmd"
        | "xfe" | "gentoo" | "rox-filer" | "polo-file-manager"
    ) { return Category::FileManager; }

    // Design / Graphics
    if matches!(c.as_str(),
        "gimp" | "gimp-2.10" | "inkscape" | "krita"
        | "figma-linux" | "penpot" | "lunacy"
        | "aseprite" | "libresprite" | "pixelorama" | "grafx2"
        | "drawio" | "dia" | "mypaint" | "xpaint" | "kolourpaint"
        | "pinta" | "tuxpaint" | "gravit-designer"
    ) { return Category::Design; }

    // Photography
    if matches!(c.as_str(),
        "darktable" | "rawtherapee" | "digikam" | "shotwell"
        | "gthumb" | "eog" | "feh" | "sxiv" | "nsxiv" | "imv"
        | "loupe" | "gpick" | "color-picker"
    ) { return Category::Photography; }

    // Video production
    if matches!(c.as_str(),
        "obs" | "obs-studio" | "obs64"
        | "kdenlive" | "shotcut" | "openshot" | "pitivi"
        | "davinci-resolve" | "resolve" | "handbrake" | "ghb"
        | "avidemux" | "flowblade" | "olive-editor" | "cinelerra"
        | "natron" | "synfig" | "synfigstudio"
    ) { return Category::Video; }

    // Gaming
    if matches!(c.as_str(),
        "steam" | "steamwebhelper"
        | "lutris" | "gamescope" | "heroic" | "heroicgameslauncher"
        | "bottles" | "itch" | "prismlauncher" | "multimc"
        | "retroarch" | "yuzu" | "ryujinx" | "cemu" | "dolphin-emu"
        | "citra" | "desmume" | "ppsspp" | "pcsx2" | "rpcs3"
        | "mangohud" | "goverlay" | "protonup-qt"
        | "minecraft-launcher" | "hmcl"
    ) { return Category::Gaming; }

    // System / Settings
    if matches!(c.as_str(),
        "gnome-control-center" | "systemsettings" | "systemsettings5"
        | "pavucontrol" | "nm-connection-editor" | "nm-applet"
        | "blueman-manager" | "blueman-applet"
        | "gnome-disks" | "baobab" | "gnome-system-monitor"
        | "htop" | "btop" | "ksysguard" | "stacer" | "mission-center"
        | "gnome-tweaks" | "dconf-editor" | "lxappearance"
        | "arandr" | "wdisplays" | "nwg-displays" | "kanshi"
        | "gparted" | "partitionmanager" | "timeshift"
        | "pamac-manager" | "synaptic" | "gnome-software" | "discover"
        | "firewall-config" | "gufw" | "seahorse" | "gnome-keyring"
    ) { return Category::System; }

    // Documents / Notes / Knowledge
    if matches!(c.as_str(),
        "obsidian" | "logseq" | "notion" | "notion-app" | "anytype"
        | "joplin" | "standard notes" | "simplenote" | "zettlr"
        | "calibre" | "calibre-gui" | "foliate"
        | "evince" | "okular" | "zathura" | "mupdf" | "xreader"
        | "atril" | "qpdfview" | "sioyek"
    ) { return Category::Documents; }

    // Office suites
    if c.contains("libreoffice") || c.contains("soffice")
        || matches!(c.as_str(), "onlyoffice" | "wps" | "calligra" | "abiword" | "gnumeric")
    { return Category::Office; }

    // 3D / CAD
    if matches!(c.as_str(),
        "blender" | "freecad" | "openscad" | "librecad"
        | "solvespace" | "sweethome3d" | "godot" | "godot4"
        | "unity" | "unreal" | "o3de"
    ) { return Category::Modeling; }

    // Torrent / Download
    if matches!(c.as_str(),
        "transmission-gtk" | "transmission-qt" | "qbittorrent"
        | "deluge" | "deluge-gtk" | "fragments" | "ktorrent"
        | "aria2" | "jdownloader" | "uget" | "persepolis"
    ) { return Category::Torrent; }

    // Virtual machines
    if matches!(c.as_str(),
        "virt-manager" | "virtualbox" | "vmware" | "vmplayer"
        | "gnome-boxes" | "qemu" | "looking-glass-client"
        | "distrobox" | "waydroid"
    ) { return Category::Vm; }

    // Science / Math
    if matches!(c.as_str(),
        "octave" | "rstudio" | "spyder" | "jupyter"
        | "geogebra" | "wxmaxima" | "scilab" | "matlab"
        | "paraview" | "gnuplot_qt"
    ) { return Category::Science; }

    Category::Unknown
}

// ── Combo rules ──────────────────────────────────────────────────────────────

/// Given the category distribution on a workspace, produce a smart name.
fn combo_name(cats: &HashMap<Category, u32>, total_windows: u32) -> String {
    if cats.is_empty() { return String::new(); }

    // Sort categories by count (descending), break ties by label alphabetically
    let mut sorted: Vec<(Category, u32)> = cats.iter().map(|(&c, &n)| (c, n)).collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.label().cmp(b.0.label())));

    let dominant = sorted[0].0;
    let dominant_count = sorted[0].1;
    let has = |c: Category| cats.contains_key(&c);
    let cat_count = cats.len();

    // ── Special combos (checked first) ───────────────────────────────────

    // Dev combos: editor + terminal = "dev"
    if has(Category::Editor) && has(Category::Terminal) {
        if has(Category::Browser) { return "fullstack".into(); }
        return "dev".into();
    }

    // Research: browser + docs/notes
    if has(Category::Browser) && has(Category::Documents) {
        return "research".into();
    }

    // Social: chat + browser (checking socials)
    if has(Category::Chat) && has(Category::Browser) {
        return "social".into();
    }

    // Streaming: media + chat (watching together / streaming)
    if has(Category::Media) && has(Category::Chat) {
        return "hangout".into();
    }

    // Content creation: video + browser (uploading/researching)
    if has(Category::Video) && has(Category::Browser) {
        return "production".into();
    }

    // Creative: design + media (designing with reference material)
    if has(Category::Design) && (has(Category::Browser) || has(Category::Media)) {
        return "creative".into();
    }

    // Gaming session: gaming + chat
    if has(Category::Gaming) && has(Category::Chat) {
        return "gaming session".into();
    }

    // Office work: office + mail
    if has(Category::Office) && has(Category::Mail) {
        return "work".into();
    }

    // Sysadmin: terminal + system
    if has(Category::Terminal) && has(Category::System) {
        return "sysadmin".into();
    }

    // Writing: editor + documents
    if has(Category::Editor) && has(Category::Documents) {
        return "writing".into();
    }

    // ── Scale modifiers ──────────────────────────────────────────────────

    // Single dominant category
    if cat_count == 1 || (dominant_count as f32 / total_windows as f32) > 0.7 {
        return scale_name(dominant, dominant_count);
    }

    // Two categories, roughly equal
    if cat_count == 2 {
        let a = sorted[0].0;
        let b = sorted[1].0;
        return format!("{} + {}", a.label(), b.label());
    }

    // Many categories — name by dominant
    if cat_count >= 3 {
        return format!("{} + {}", dominant.label(), (cat_count - 1));
    }

    dominant.label().into()
}

/// Apply scale modifiers for when many windows of the same type are open.
fn scale_name(cat: Category, count: u32) -> String {
    match count {
        0 => String::new(),
        1 => cat.label().into(),
        2 => cat.label().into(),
        3..=4 => cat.plural().into(),
        5..=7 => format!("{} farm", cat.label()),
        _ => format!("mega {}", cat.label()),
    }
}

// ── Namer state ──────────────────────────────────────────────────────────────

pub struct NamingRule {
    pub classes: Vec<String>,
    pub name: String,
}

#[derive(Default)]
pub struct WorkspaceNamer {
    pub enabled: bool,
    pub rules: Vec<NamingRule>,
}

impl WorkspaceNamer {
    /// Compute display names for workspaces in-place.
    /// Respects manual pins from woven.store, then user rules, then auto-classification.
    /// Also records name assignments in the frequency table for learning.
    pub fn apply_names(
        &self,
        workspaces: &mut [Workspace],
        store: &std::sync::Mutex<HashMap<String, serde_json::Value>>,
    ) {
        if !self.enabled { return; }

        let Ok(mut store_guard) = store.lock() else { return };

        for ws in workspaces.iter_mut() {
            // 1. Manual pin override
            let pin_key = format!("ws_namer.pin.{}", ws.id);
            if let Some(serde_json::Value::String(name)) = store_guard.get(&pin_key) {
                ws.name = name.clone();
                continue;
            }

            // 2. Empty workspace — keep the number
            if ws.windows.is_empty() { continue; }

            // 3. Frequency table lookup
            let class_hash = class_set_hash(&ws.windows);
            let freq_key = format!("ws_namer.freq.{}", class_hash);
            if let Some(serde_json::Value::Object(freqs)) = store_guard.get(&freq_key) {
                if let Some((best_name, _)) = freqs.iter()
                    .filter_map(|(k, v)| v.as_u64().map(|n| (k, n)))
                    .max_by_key(|(_, n)| *n)
                {
                    ws.name = best_name.clone();
                    continue;
                }
            }

            // 4. User-defined rules (checked in order)
            let ws_classes: Vec<String> = ws.windows.iter()
                .map(|w| w.class.to_lowercase())
                .collect();
            let mut matched_rule = false;
            for rule in &self.rules {
                if rule.classes.iter().all(|rc| ws_classes.iter().any(|wc| wc == rc)) {
                    ws.name = rule.name.clone();
                    matched_rule = true;
                    break;
                }
            }
            if matched_rule {
                // Record for frequency learning
                Self::record(&mut store_guard, &ws.windows, &ws.name);
                continue;
            }

            // 5. Auto-classification
            let mut cats: HashMap<Category, u32> = HashMap::new();
            let mut unknown_classes: Vec<String> = Vec::new();
            for w in &ws.windows {
                let cat = classify(&w.class);
                *cats.entry(cat).or_insert(0) += 1;
                if cat == Category::Unknown && !w.class.is_empty() {
                    unknown_classes.push(w.class.to_lowercase());
                }
            }

            // If everything is unknown, use the most common class name directly
            if cats.len() == 1 && cats.contains_key(&Category::Unknown) {
                if let Some(name) = most_common(&unknown_classes) {
                    ws.name = name;
                    Self::record(&mut store_guard, &ws.windows, &ws.name);
                    continue;
                }
            }

            // Remove unknowns from combo logic if there are known categories
            if cats.len() > 1 {
                cats.remove(&Category::Unknown);
            }

            let total = cats.values().sum();
            let name = combo_name(&cats, total);
            if !name.is_empty() {
                ws.name = name;
                Self::record(&mut store_guard, &ws.windows, &ws.name);
            }
        }
    }

    /// Record a name→class-set mapping in the frequency table (called under lock).
    fn record(
        store: &mut HashMap<String, serde_json::Value>,
        windows: &[woven_common::types::Window],
        name: &str,
    ) {
        if windows.is_empty() { return; }
        let class_hash = class_set_hash(windows);
        let freq_key = format!("ws_namer.freq.{}", class_hash);

        let entry = store.entry(freq_key).or_insert_with(|| serde_json::Value::Object(Default::default()));
        if let serde_json::Value::Object(map) = entry {
            let count = map.get(name)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            map.insert(name.into(), serde_json::Value::Number((count + 1).into()));
        }
    }
}

/// Build a stable hash key from the set of window classes on a workspace.
fn class_set_hash(windows: &[woven_common::types::Window]) -> String {
    let mut classes: Vec<String> = windows.iter()
        .map(|w| w.class.to_lowercase())
        .filter(|c| !c.is_empty())
        .collect();
    classes.sort();
    classes.dedup();
    classes.join(",")
}

/// Find the most common string in a list.
fn most_common(items: &[String]) -> Option<String> {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for item in items {
        *counts.entry(item.as_str()).or_insert(0) += 1;
    }
    counts.into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(s, _)| s.to_string())
}
