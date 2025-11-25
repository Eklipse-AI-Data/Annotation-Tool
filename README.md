# AnnotationTool

A powerful, feature-rich YOLO annotation tool with an Eclipse-inspired dark theme. Built for efficient image annotation with advanced features like crosshair cursor, grid lines, class management, and template-based drawing.

![Python](https://img.shields.io/badge/python-3.8+-blue.svg)
![License](https://img.shields.io/badge/license-MIT-green.svg)

## Features

### 🎯 Visual Drawing Aids
- **Crosshair Cursor**: Automatically appears when a class is selected for precise box placement
- **Grid Lines**: Green dashed lines follow your mouse (vertical & horizontal) for perfect alignment
- **Template Mode**: Create reusable box templates with predefined dimensions
- **Idle Mode**: Press Escape to deselect class and remove visual cues

### 📦 Annotation Capabilities
- **YOLO Format**: Full support for YOLO bounding box format (normalized coordinates)
- **Multi-Box Selection**: Select and manipulate multiple boxes simultaneously
- **Copy & Paste**: Duplicate annotations across images (Ctrl+C / Ctrl+V)
- **Box Manipulation**: Move, resize (corner handles), and delete boxes with ease
- **Auto-Save**: Automatically save annotations when switching images

### 🎨 Class Management
- **Dynamic Class Editor**: Add, remove, and reorder classes through Settings
- **Automatic Remapping**: When classes are modified, all annotation files are automatically updated
- **Backup System**: Automatic timestamped backups before making class changes
- **Color-Coded Classes**: Each class gets a distinct color for easy identification
- **Template Sizes**: Set default width/height for each class

### ⚙️ Advanced Features
- **Zoom & Pan**: Ctrl+MouseWheel to zoom, Middle-click to pan
- **Scrollbars**: Navigate large images with horizontal and vertical scrollbars
- **Configurable Keybindings**: Customize all keyboard shortcuts through Settings
- **Image Resolution Helper**: Optional downscaling for faster annotation of high-res images
- **Dark Theme**: Eclipse-inspired UI for reduced eye strain

### 🖥️ User Interface
- **Dual Sidebars**: Class list on left, box labels on right (toggle with button)
- **File Browser**: Quick navigation through image dataset
- **Box List**: View and select all annotations in current image
- **Status Display**: Current image index and filename in title bar

## Installation

### Prerequisites
- Python 3.8 or higher
- pip package manager

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

1. **Open Images**: Click "Open Images" and select your image directory
2. **Select Class**: Click a class from the list (crosshair cursor appears)
3. **Draw Boxes**: 
   - Click and drag to draw a box
   - OR click once to stamp a template box
4. **Adjust Boxes**: Click box to select, then drag to move or use corner handles to resize
5. **Save**: Annotations auto-save when switching images (or manually save)

### Keyboard Shortcuts (Default)

| Action | Shortcut |
|--------|----------|
| Next Image | `D` or `→` |
| Previous Image | `A` or `←` |
| Cycle Class | `W` |
| Delete Box | `Delete` |
| Copy Boxes | `Ctrl+C` |
| Paste Boxes | `Ctrl+V` |
| Edit Box Class | `Ctrl+E` |
| Deselect Class (Idle) | `Escape` |

*All shortcuts are customizable in Settings → Keybindings*

## Features Guide

### Creating Templates

1. Select a class
2. Click "Create Template"
3. Draw a box with your desired dimensions
4. The class will remember this size for quick stamping

### Managing Classes

1. Click "Settings" → "Class Management" tab
2. **Add**: Enter name and click "Add Class"
3. **Remove**: Select class(es) and click "Remove Selected"
4. **Reorder**: Use "Move Up" / "Move Down" buttons
5. **Apply**: Click "Apply Changes" to update all annotation files

> ⚠️ **Important**: Removing or reordering classes will automatically update all `.txt` files in your image directory. A backup is created before changes.

### Using Grid Lines

1. Select any class from the list
2. Move your mouse over the canvas
3. Green crosshair grid lines appear to help align boxes
4. Press `Escape` to remove grid lines (enters Idle mode)

### Zoom and Pan

- **Zoom**: Hold `Ctrl` + scroll mouse wheel
- **Pan**: Click and drag with middle mouse button
- **Reset**: Reload the image to reset zoom/pan

## File Structure

```
AnnotationTool/
├── main.py                          # Application entry point
├── data/
│   └── predefined_classes.txt       # Class definitions (one per line)
├── src/
│   ├── __init__.py
│   ├── app.py                       # Main application logic
│   ├── ui_components.py             # Dark theme UI components
│   └── utils.py                     # Helper functions (YOLO parsing, etc.)
└── config.json                      # Keybinding configuration (auto-generated)
```

## YOLO Format

Annotations are saved in YOLO format:
```
<class_id> <x_center> <y_center> <width> <height>
```

All coordinates are normalized (0-1 range):
- `class_id`: Integer index from predefined_classes.txt
- `x_center`, `y_center`: Center point of bounding box
- `width`, `height`: Box dimensions

Example:
```
0 0.5 0.5 0.2 0.3
1 0.3 0.7 0.15 0.25
```

## Configuration

### Predefined Classes

Edit `data/predefined_classes.txt` (one class per line):
```
blue_ace
blue_baron
blue_dragon
health_bar_high
health_bar_low
map
victory
defeat
```

Or use Settings → Class Management for a GUI editor.

### Keybindings

Keybindings are stored in `config.json`. Use Settings → Keybindings to customize.

## Tips & Tricks

- **Batch Annotation**: Use Copy/Paste to duplicate boxes across similar images
- **Template Workflow**: Create templates for common object sizes to speed up annotation
- **Idle Mode**: Deselect class (Escape) to navigate without accidentally drawing boxes
- **Grid Lines**: Use for aligning UI elements or objects that need precise positioning
- **Auto-Save**: Keep enabled to never lose work when switching images

## Troubleshooting

### Grid lines not appearing
- Make sure a class is selected (not in Idle mode)
- Check that you're moving the mouse over the canvas area

### Annotations not saving
- Verify "Auto Save" is checked in the sidebar
- Ensure output directory is set (defaults to image directory)
- Check file permissions in the output directory

### Classes not updating
- After modifying `predefined_classes.txt`, restart the application
- Or use Settings → Class Management → Apply Changes

## Contributing

Contributions are welcome! Please feel free to submit pull requests or open issues for bugs and feature requests.

## License

This project is licensed under the MIT License.

## Acknowledgments

- Built with Python and Tkinter
- Inspired by Eclipse IDE's dark theme
- Designed for YOLO object detection workflows
