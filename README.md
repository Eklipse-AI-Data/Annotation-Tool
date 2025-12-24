# AnnotationTool (Midnight Glass)

A powerful, feature-rich YOLO annotation tool with a professional MVC architecture and premium themes like "Midnight Glass". Built for high-speed image annotation with advanced features like crosshair cursor, grid lines, class management, game presets, image preloading, and batch operations.

![Python](https://img.shields.io/badge/python-3.8+-blue.svg)
![License](https://img.shields.io/badge/license-MIT-green.svg)

## Highlights

### 🎨 Premium UI & Themes

- **Midnight Glass UI**: Modern flat dark theme designed for long labeling sessions.
- **Multi-Theme Support**: Choose from various professional palettes (Midnight Glass, Anthropic, Emerald, Ruby, Amber, Frost).
- **Glassmorphic Elements**: Subtle transparencies and sleek borders for a premium feel.
- **Responsive Canvas**: Ultra-smooth zooming and panning even with high-resolution images.

### 🎮 Game Presets & Class Management

- **Game Presets**: Instant switching between different class sets (e.g., Fortnite, Warzone, Arc Raiders) via `Settings > Game Presets`.
- **Dynamic Class Editor**: Add, remove, and reorder classes through a dedicated GUI.
- **Automatic Remapping**: All annotation files are automatically updated when classes are modified or reordered.
- **Backups**: Automatic timestamped backups are created before any major class change.

### ⚙️ Power Tools

- **Batch Operations**: Replace all instances of one Class ID with another across your entire dataset in seconds.
- **Search & Filtering**: Search through class lists and filter your image set to show only images containing specific labels.
- **Image Preloading**: Multi-threaded preloading for instant navigation between high-res frames.
- **Template Mode**: Define standard box sizes for repeatable object types to stamp annotations instantly.
- **Validation tool**: Check for potential annotation errors or empty files.

## Features

### 🎯 Visual Drawing Aids

- **Crosshair Cursor**: High-precision targeting for box placement.
- **Grid Lines**: Vertical and horizontal dashed lines follow the mouse for perfect alignment.
- **Idle Mode**: Press `Escape` to enter "Peaceful Mode," removing visual helpers for simple image review.

### 📦 Annotation Capabilities

- **YOLO Format**: Native support for normalized YOLO `.txt` files.
- **Multi-Box Interaction**: Select multiple boxes to move, delete, or reassign classes in bulk.
- **Clipboard Support**: `Ctrl+C` and `Space` (or `Ctrl+V`) to quickly duplicate annotations across frames.
- **Undo/Redo**: Deep history stack for all annotation operations.
- **Auto-Save**: Changes are saved instantly as you move through your dataset.

## Installation

### Prerequisites

- Python 3.8 or higher
- `Pillow` library

### Setup

1. **Clone the repository**

```bash
git clone https://github.com/Eklipse-AI-Data/Annotation-Tool.git
cd Annotation-Tool
```

2. **Install dependencies**

```bash
pip install pillow
```

3. **Run the application**

```bash
python main.py
```

## Quick Start

### Basic Workflow

1. **Open Images**: Click "Open Images" and select your image directory.
2. **Setup Classes**: Go to `Settings > Game Presets` to load a predefined game file or create your own.
3. **Select Class**: Click a class from the left sidebar list.
4. **Draw Boxes**:
   - Click and drag for custom sizes.
   - OR click once to use your defined **Template**.
5. **Navigate**: Use keyboard shortcuts to flip through images; they will auto-save.

### Keyboard Shortcuts (Latest Defaults)

| Action                    | Shortcut               |
| ------------------------- | ---------------------- |
| Next Image                | `D` or `→`             |
| Previous Image            | `A` or `←`             |
| Cycle Class               | `Down Arrow`           |
| Delete Box                | `Q` or `Delete`        |
| Copy Boxes                | `Ctrl+C`               |
| Paste Boxes               | `Space`                |
| Undo/Redo                 | `Ctrl+Z` / `Ctrl+Y`    |
| Apply Preset Slot         | `Ctrl + [1-9]`         |
| Save Preset Slot          | `Ctrl + Shift + [1-9]` |
| Toggle Unannotated Filter | `U`                    |
| Show Help Overlay         | `?`                    |
| Edit Box Class            | `Ctrl+E`               |
| Deselect Class (Idle)     | `Escape`               |

_Note: You can customize every single key in Settings → Keybindings._

## Features Guide

### 📂 Game Presets (Switching Games)

If you work on multiple games, you don't need to manually rename files:

1. Open **Settings** (Bottom Left).
2. Go to the **Game Presets** tab.
3. Select a game from the list (sourced from the `data/` folder).
4. Click **Load Selected Game**. The tool will offer to backup your current `predefined_classes.txt` first.

### 🔄 Batch Replacing IDs

Found out you labeled "Enemy" as ID 0 when it should have been ID 5?

1. Open **Settings** → **Batch Operations**.
2. Select the "Old ID" (0) and the "New ID" (5).
3. Click **Execute**. The tool will scan all `.txt` files in your output directory and update them instantly.

### 🔍 Filtering Images by Class

Working on a dataset of 10,000 images but only want to see "Kill Feeds"?

1. Select the class in the **Filter** dropdown above the image list.
2. Click **Filter**.
3. The image list will now only show frames containing that specific label.

## File Structure

```
AnnotationTool/
├── main.py                          # Application entry point
├── config.json                      # User settings and keybindings
├── session.json                     # Persistent session state (recent projects, theme, etc.)
├── data/                            # Game class presets (.txt files)
├── Images/                          # Default directory for project images
└── src/
    ├── model.py                     # Data handling & annotation logic
    ├── view.py                      # UI layout & rendering
    ├── controller.py                # Event handling & app flow
    ├── themes.py                    # Predefined color palettes
    ├── ui_components.py             # Reusable custom UI widgets
    └── utils.py                     # YOLO parsing, backups, & image processing
```

## Contributing

Contributions are welcome! This tool is optimized for professional gaming dataset creators.

## License

This project is licensed under the MIT License.
