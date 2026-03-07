# 🕸️ Woven

![Hyprland Required](https://img.shields.io/badge/Hyprland-Required-00adff?style=flat-square&logo=hyprland&logoColor=white)
![Status: Work in Progress](https://img.shields.io/badge/Status-Work_in_Progress-orange)
![Status](https://img.shields.io/badge/Status-Due%20to%20Change-orange?style=flat-square)
![Platform: Wayland | X11](https://img.shields.io/badge/Platform-Wayland%20%7C%20X11-blue)
![Language: Rust | Lua](https://img.shields.io/badge/Language-Rust%20%7C%20Lua-red)

**Woven** is an up-and-coming, highly customizable central overlay for window managers. Built primarily for **Wayland** (with portable **X11** support), Woven gives you a new version of niri's overlay system  

Whether you are a power user needing to find something and manage your usage or a rice enthusiast looking for the perfect niri overlay replacement, Woven acts as a user-friendly tool that will help assist you, and talor it to your needs with the declarative file that we have a program to edit with just to make it nicer to newbies.

---

## ✨ Core Features

* **The Bird's Eye View:** Summons a sleek, centralized overlay over your current window manager, providing instant access to everything happening on your machine.
* **Deep System Monitoring:** See total processing power, memory usage, and system thermals at a glance.
* **Per-Program Telemetry:** Break down your resource usage program-by-program to see exactly what is eating your CPU or battery.
* **Wayland First, X11 Friendly:** Designed from the ground up to play nicely with modern Wayland compositors (Hyprland, Sway, etc.) while maintaining backward compatibility with X11 setups.

## 🏗️ Architecture & Tech Stack

Woven is built for blazing speed and ultimate flexibility by splitting its responsibilities:

* **The Engine (Rust):** The heavy lifting—system monitoring, window drawing, and process management—is written entirely in Rust for memory safety, low overhead, and maximum performance.
* **The Blueprint (Lua):** Woven is configured and guided entirely via **Lua**. Lua acts as the declarative core, allowing you to script, theme, and mold the overlay exactly to your liking without needing to recompile the project.
* **The Manager (Tool):** A standalone management program handles the Woven lua configuration, allowing you to change it whenever you like and with nice prompts and settings buttons for easy control and management with it. Allows themeing and customizing the logos and style used in it.

## 🚀 Roadmap (Coming Soon)

Woven is currently in early development. Here is what we are working on:

- [ ] **Core Daemon:** Establish Rust backend for basic overlay rendering.
- [ ] **Process Monitoring:** Implement real-time CPU/RAM polling per program.
- [ ] **Seperate program to overview:** Make a different program to manage it, keeping the controller and management only tied by the single lua file 
- [ ] **Wayland/X11 Compositing:** Ensure seamless transparency and blur effects across different display servers.
- [ ] **Full GUI Configuration Tool:** A complete, user-friendly graphical interface to install, set up, and configure Woven without ever touching a config file (for those who prefer a GUI over code).
- [ ] **Plugin Ecosystem:** Allow community-made widgets to be injected into the overlay.

## 🛠️ Getting Started (Placeholder)

> **Note:** Woven is currently a placeholder/WIP. The only provided file is the current code of it. This is mainly an emergency backup for **MY** use and may not work on other systems 

### Prerequisites
* `rustc` and `cargo` (latest stable)
* `lua5.1` or `luajit`
* Wayland compositor (e.g., Hyprland) or X11 Window Manager
* Hyprland

### Installation
**NOT** Releasable currently
Services and other things arent currently released with the compressed package, buisness releases for the future
