import re
import os
import json
import random
import colorsys

def load_classes(file_path):
    """
    Loads classes from a text file, one per line.
    Returns a list of dicts: {'id': int, 'name': str, 'color': str, 'default_w': 100, 'default_h': 100}
    """
    classes = []
    if not os.path.exists(file_path):
        return classes
        
    try:
        with open(file_path, 'r') as f:
            for i, line in enumerate(f):
                name = line.strip()
                if name:
                    color = generate_color(i)
                    classes.append({
                        'id': i,
                        'name': name,
                        'color': color,
                        'default_w': 100, # Default, can be updated by user
                        'default_h': 100
                    })
    except Exception as e:
        print(f"Error loading classes: {e}")
    return classes

def load_config(path):
    default_config = {
        "deselect": "<Escape>",
        "next_image": "<d>",
        "prev_image": "<a>",
        "cycle_class": "<w>",
        "delete_box": "<Delete>",
        "copy": "<Control-c>",
        "paste": "<Control-v>",
        "edit_class": "<Control-e>"
    }
    if not os.path.exists(path):
        return default_config
    
    try:
        with open(path, 'r') as f:
            user_config = json.load(f)
            # Merge with defaults to ensure all keys exist
            config = default_config.copy()
            config.update(user_config)
            return config
    except Exception as e:
        print(f"Error loading config: {e}")
        return default_config

def save_config(path, config):
    try:
        with open(path, 'w') as f:
            json.dump(config, f, indent=4)
    except Exception as e:
        print(f"Error saving config: {e}")

def natural_sort_key(s):
    """
    Key function for natural sorting of filenames.
    e.g., img1.jpg, img2.jpg, img10.jpg
    """
    return [int(text) if text.isdigit() else text.lower()
            for text in re.split('([0-9]+)', s)]

def generate_color(class_id):
    """
    Generates a distinct color for a given class ID.
    Returns a hex string (e.g., '#RRGGBB').
    """
    # Use golden ratio conjugate to spread hues evenly
    golden_ratio_conjugate = 0.618033988749895
    hue = (class_id * golden_ratio_conjugate) % 1.0
    # High saturation and value for visibility
    saturation = 0.8
    value = 0.95
    
    rgb = colorsys.hsv_to_rgb(hue, saturation, value)
    r, g, b = [int(x * 255) for x in rgb]
    return f'#{r:02x}{g:02x}{b:02x}'

def parse_yolo(file_path, img_width, img_height):
    """
    Parses a YOLO format .txt file.
    Returns a list of dicts: {'class_id': int, 'x': float, 'y': float, 'w': float, 'h': float}
    Coordinates in the returned dict are NORMALIZED (0-1).
    """
    boxes = []
    if not os.path.exists(file_path):
        return boxes

    try:
        with open(file_path, 'r') as f:
            for line in f:
                parts = line.strip().split()
                if len(parts) >= 5:
                    class_id = int(parts[0])
                    x_center = float(parts[1])
                    y_center = float(parts[2])
                    w = float(parts[3])
                    h = float(parts[4])
                    
                    boxes.append({
                        'class_id': class_id,
                        'x_center': x_center,
                        'y_center': y_center,
                        'w': w,
                        'h': h
                    })
    except Exception as e:
        print(f"Error parsing YOLO file {file_path}: {e}")
        
    return boxes

def save_yolo(file_path, boxes):
    """
    Saves a list of boxes to a YOLO format .txt file.
    Boxes should be a list of dicts with keys: class_id, x_center, y_center, w, h (normalized).
    """
    try:
        with open(file_path, 'w') as f:
            for box in boxes:
                line = f"{box['class_id']} {box['x_center']:.6f} {box['y_center']:.6f} {box['w']:.6f} {box['h']:.6f}\n"
                f.write(line)
    except Exception as e:
        print(f"Error saving YOLO file {file_path}: {e}")

def denormalize_box(box, img_width, img_height):
    """
    Convert normalized YOLO coordinates (center_x, center_y, w, h) to pixel coordinates (x1, y1, x2, y2).
    """
    w = box['w'] * img_width
    h = box['h'] * img_height
    x_center = box['x_center'] * img_width
    y_center = box['y_center'] * img_height
    
    x1 = x_center - (w / 2)
    y1 = y_center - (h / 2)
    x2 = x_center + (w / 2)
    y2 = y_center + (h / 2)
    
    return x1, y1, x2, y2

def normalize_box(x1, y1, x2, y2, img_width, img_height):
    """
    Convert pixel coordinates (x1, y1, x2, y2) to normalized YOLO coordinates (center_x, center_y, w, h).
    """
    w = abs(x2 - x1)
    h = abs(y2 - y1)
    x_center = min(x1, x2) + (w / 2)
    y_center = min(y1, y2) + (h / 2)
    
    return {
        'x_center': x_center / img_width,
        'y_center': y_center / img_height,
        'w': w / img_width,
        'h': h / img_height
    }
