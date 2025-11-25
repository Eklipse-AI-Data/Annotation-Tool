import re
import os
import json
import random
import colorsys
from PIL import Image

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

def save_classes(file_path, classes):
    """
    Saves classes to a text file, one per line.
    
    Args:
        file_path (str): Path to the classes file.
        classes (list): List of class name strings.
    """
    try:
        with open(file_path, 'w') as f:
            for class_name in classes:
                f.write(f"{class_name}\n")
    except Exception as e:
        print(f"Error saving classes: {e}")

def create_class_mapping(old_classes, new_classes):
    """
    Creates a mapping from old class IDs to new class IDs.
    
    Args:
        old_classes (list): List of old class names.
        new_classes (list): List of new class names.
    
    Returns:
        dict: Mapping of old class ID to new class ID. Returns None for removed classes.
    """
    mapping = {}
    for old_id, old_name in enumerate(old_classes):
        if old_name in new_classes:
            new_id = new_classes.index(old_name)
            mapping[old_id] = new_id
        else:
            mapping[old_id] = None  # Class was removed
    return mapping

def update_annotation_file(file_path, class_mapping):
    """
    Updates a single annotation file with new class IDs based on the mapping.
    
    Args:
        file_path (str): Path to the annotation .txt file.
        class_mapping (dict): Mapping of old class ID to new class ID.
    
    Returns:
        bool: True if successful, False otherwise.
    """
    if not os.path.exists(file_path):
        return True  # Nothing to update
    
    try:
        updated_lines = []
        with open(file_path, 'r') as f:
            for line in f:
                parts = line.strip().split()
                if len(parts) >= 5:
                    old_class_id = int(parts[0])
                    new_class_id = class_mapping.get(old_class_id)
                    
                    if new_class_id is not None:
                        # Update the class ID
                        parts[0] = str(new_class_id)
                        updated_lines.append(' '.join(parts) + '\n')
                    # If new_class_id is None, skip this line (class was removed)
        
        # Write updated content back to file
        with open(file_path, 'w') as f:
            f.writelines(updated_lines)
        
        return True
    except Exception as e:
        print(f"Error updating annotation file {file_path}: {e}")
        return False

def backup_annotations(image_dir):
    """
    Creates a backup of all annotation files in the image directory.
    
    Args:
        image_dir (str): Path to the directory containing images and annotations.
    
    Returns:
        str: Path to the backup directory, or None if failed.
    """
    from datetime import datetime
    
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    backup_dir = os.path.join(image_dir, f"annotations_backup_{timestamp}")
    
    try:
        os.makedirs(backup_dir, exist_ok=True)
        
        backup_count = 0
        for filename in os.listdir(image_dir):
            if filename.lower().endswith('.txt'):
                src = os.path.join(image_dir, filename)
                dst = os.path.join(backup_dir, filename)
                
                # Copy the file
                with open(src, 'r') as f_src:
                    content = f_src.read()
                with open(dst, 'w') as f_dst:
                    f_dst.write(content)
                
                backup_count += 1
        
        if backup_count > 0:
            print(f"Backed up {backup_count} annotation files to {backup_dir}")
            return backup_dir
        else:
            print("No annotation files found to backup")
            return None
            
    except Exception as e:
        print(f"Error creating backup: {e}")
        return None


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

def resize_images_to_lowres(input_folder, target_width=720):
    """
    Resizes images in a folder to lower resolution and saves them to a new directory.
    
    Args:
        input_folder (str): Path to the folder containing the original images.
        target_width (int): Target width for the intermediate resize (default: 720).
    
    Returns:
        str: Path to the lowres directory, or None if operation failed.
    """
    # Create output folder path
    output_folder = input_folder.rstrip(os.sep) + "_lowres"
    
    # Create output directory if it doesn't exist
    if not os.path.exists(output_folder):
        os.makedirs(output_folder)
    
    processed_count = 0
    error_count = 0
    
    try:
        for filename in os.listdir(input_folder):
            if filename.lower().endswith((".png", ".jpg", ".jpeg")):
                try:
                    # Open the image
                    img_path = os.path.join(input_folder, filename)
                    img = Image.open(img_path)
                    
                    # Resize to target width (maintaining aspect ratio)
                    target_height = int(target_width * img.height / img.width)
                    img_resized = img.resize((target_width, target_height), Image.LANCZOS)
                    
                    # Resize back to 1920x1080
                    img_final = img_resized.resize((1920, 1080), Image.LANCZOS)
                    
                    # Save the image
                    output_path = os.path.join(output_folder, filename)
                    # Save as JPEG for consistency
                    if filename.lower().endswith(".png"):
                        # Convert RGBA to RGB if necessary
                        if img_final.mode == 'RGBA':
                            img_final = img_final.convert('RGB')
                        img_final.save(output_path, "JPEG")
                    else:
                        img_final.save(output_path, "JPEG")
                    
                    processed_count += 1
                    
                except Exception as e:
                    print(f"Error processing {filename}: {e}")
                    error_count += 1
        
        if processed_count > 0:
            print(f"Successfully processed {processed_count} images to {output_folder}")
            if error_count > 0:
                print(f"Failed to process {error_count} images")
            return output_folder
        else:
            print("No images were processed")
            return None
            
    except Exception as e:
        print(f"Error accessing folder: {e}")
        return None
