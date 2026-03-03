import re
import os
import json
import random
import colorsys
import threading
from collections import OrderedDict
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
        "next_image": "<Right>",
        "prev_image": "<Left>",
        "cycle_class": "<Down>",
        "delete_box": "<Delete>",
        "copy": "<Control-c>",
        "paste": "<Control-v>",
        "edit_class": "<Control-e>",
        "undo": "<Control-z>",
        "redo": "<Control-y>"
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

# ==================== SESSION MANAGEMENT ====================

def get_default_session():
    """Returns default session state."""
    return {
        "last_image_dir": "",
        "last_output_dir": "",
        "last_image_index": 0,
        "zoom_factor": 1.0,
        "show_labels": True,
        "auto_save": True,
        "show_right_sidebar": True
    }

def load_session(path):
    """
    Load last session state from JSON file.
    Returns default session if file doesn't exist or is corrupted.
    """
    default = get_default_session()
    if not os.path.exists(path):
        return default
    
    try:
        with open(path, 'r') as f:
            session = json.load(f)
            # Merge with defaults to ensure all keys exist
            merged = default.copy()
            merged.update(session)
            return merged
    except Exception as e:
        print(f"Error loading session: {e}")
        return default

def save_session(path, session_state):
    """Save session state to JSON file."""
    try:
        with open(path, 'w') as f:
            json.dump(session_state, f, indent=4)
    except Exception as e:
        print(f"Error saving session: {e}")

def get_annotated_images(image_dir, output_dir, image_list):
    """
    Returns a set of image filenames that have corresponding annotation files.
    
    Args:
        image_dir: Directory containing images
        output_dir: Directory containing annotation .txt files
        image_list: List of image filenames to check
    
    Returns:
        set: Set of image filenames that have annotations
    """
    annotated = set()
    if not output_dir or not os.path.exists(output_dir):
        return annotated
    
    for img_file in image_list:
        name, _ = os.path.splitext(img_file)
        txt_path = os.path.join(output_dir, name + ".txt")
        if os.path.exists(txt_path):
            # Check if file is not empty
            try:
                if os.path.getsize(txt_path) > 0:
                    annotated.add(img_file)
            except:
                pass
    
    return annotated

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

def update_annotation_file(file_path, class_mapping, remove_unmapped=False):
    """
    Updates a single annotation file with new class IDs based on the mapping.
    
    Args:
        file_path (str): Path to the annotation .txt file.
        class_mapping (dict): Mapping of old class ID to new class ID.
        remove_unmapped (bool): If True, remove lines with class IDs not in mapping.
                               If False, keep them unchanged (default for batch replace).
    
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
                    
                    if old_class_id in class_mapping:
                        new_class_id = class_mapping[old_class_id]
                        if new_class_id is not None:
                            # Update the class ID
                            parts[0] = str(new_class_id)
                            updated_lines.append(' '.join(parts) + '\n')
                        # If new_class_id is None, skip this line (class was removed)
                    elif not remove_unmapped:
                        # Keep the line unchanged if not in mapping and remove_unmapped is False
                        updated_lines.append(' '.join(parts) + '\n')
                    # If remove_unmapped is True and not in mapping, skip this line
        
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


# ==================== IMAGE PRELOADER ====================

class ImagePreloader:
    """
    Background image preloader for smoother navigation.
    Caches adjacent images to reduce load times when navigating.
    """
    def __init__(self, cache_size=7):
        self.cache = OrderedDict()
        self.cache_size = cache_size
        self.lock = threading.Lock()
        self._loading = set()  # Track images currently being loaded
    
    def get(self, image_path):
        """Get an image from cache, or None if not cached."""
        with self.lock:
            if image_path in self.cache:
                # Move to end (most recently used)
                self.cache.move_to_end(image_path)
                return self.cache[image_path]
        return None
    
    def put(self, image_path, image):
        """Add an image to the cache."""
        with self.lock:
            if image_path in self.cache:
                self.cache.move_to_end(image_path)
            else:
                self.cache[image_path] = image
                # Evict oldest if over capacity
                while len(self.cache) > self.cache_size:
                    self.cache.popitem(last=False)
    
    def preload(self, image_paths):
        """Preload multiple images in background threads."""
        for path in image_paths:
            if path not in self.cache and path not in self._loading:
                self._loading.add(path)
                thread = threading.Thread(target=self._load_image, args=(path,), daemon=True)
                thread.start()
    
    def _load_image(self, image_path):
        """Load a single image (runs in background thread)."""
        try:
            if os.path.exists(image_path):
                img = Image.open(image_path)
                img.load()  # Force load into memory
                self.put(image_path, img)
        except Exception as e:
            print(f"Preload error for {image_path}: {e}")
        finally:
            with self.lock:
                self._loading.discard(image_path)
    
    def clear(self):
        """Clear the cache."""
        with self.lock:
            self.cache.clear()


# ==================== ANNOTATION VALIDATION ====================

def validate_annotations(boxes, classes, image_width=None, image_height=None):
    """
    Validate annotation boxes for common issues.
    
    Args:
        boxes: List of box dicts with x_center, y_center, w, h, class_id
        classes: List of class dicts with id and name
        image_width: Optional image width for additional checks
        image_height: Optional image height for additional checks
    
    Returns:
        List of warning dicts: {'type': str, 'message': str, 'box_index': int, 'severity': str}
    """
    warnings = []
    class_ids = {c['id'] for c in classes}
    
    for i, box in enumerate(boxes):
        # Check for invalid class ID
        if box['class_id'] not in class_ids and box['class_id'] != -1:
            warnings.append({
                'type': 'invalid_class',
                'message': f"Box {i}: Unknown class ID {box['class_id']}",
                'box_index': i,
                'severity': 'error'
            })
        
        # Check for out of bounds (normalized should be 0-1)
        if not (0 <= box['x_center'] <= 1) or not (0 <= box['y_center'] <= 1):
            warnings.append({
                'type': 'out_of_bounds',
                'message': f"Box {i}: Center position out of bounds",
                'box_index': i,
                'severity': 'error'
            })
        
        # Check if box extends outside image
        x1 = box['x_center'] - box['w'] / 2
        y1 = box['y_center'] - box['h'] / 2
        x2 = box['x_center'] + box['w'] / 2
        y2 = box['y_center'] + box['h'] / 2
        
        if x1 < -0.01 or y1 < -0.01 or x2 > 1.01 or y2 > 1.01:
            warnings.append({
                'type': 'extends_outside',
                'message': f"Box {i}: Extends outside image boundaries",
                'box_index': i,
                'severity': 'warning'
            })
        
        # Check for tiny boxes (likely accidental clicks)
        area = box['w'] * box['h']
        if area < 0.0001:  # Less than 0.01% of image
            warnings.append({
                'type': 'tiny_box',
                'message': f"Box {i}: Extremely small ({area*100:.4f}% of image)",
                'box_index': i,
                'severity': 'warning'
            })
        
        # Check for very large boxes (might be errors)
        if area > 0.9:  # More than 90% of image
            warnings.append({
                'type': 'huge_box',
                'message': f"Box {i}: Covers {area*100:.1f}% of image",
                'box_index': i,
                'severity': 'info'
            })
        
        # Check for boxes with zero dimension
        if box['w'] <= 0 or box['h'] <= 0:
            warnings.append({
                'type': 'zero_dimension',
                'message': f"Box {i}: Has zero or negative dimension",
                'box_index': i,
                'severity': 'error'
            })
    
    # Check for overlapping boxes (same class, high IoU)
    for i in range(len(boxes)):
        for j in range(i + 1, len(boxes)):
            if boxes[i]['class_id'] == boxes[j]['class_id']:
                iou = calculate_iou(boxes[i], boxes[j])
                if iou > 0.9:
                    warnings.append({
                        'type': 'duplicate',
                        'message': f"Boxes {i} and {j}: Possible duplicates (IoU={iou:.2f})",
                        'box_index': i,
                        'severity': 'warning'
                    })
    
    return warnings


def calculate_iou(box1, box2):
    """Calculate Intersection over Union between two normalized boxes."""
    # Convert to corner format
    x1_1 = box1['x_center'] - box1['w'] / 2
    y1_1 = box1['y_center'] - box1['h'] / 2
    x2_1 = box1['x_center'] + box1['w'] / 2
    y2_1 = box1['y_center'] + box1['h'] / 2
    
    x1_2 = box2['x_center'] - box2['w'] / 2
    y1_2 = box2['y_center'] - box2['h'] / 2
    x2_2 = box2['x_center'] + box2['w'] / 2
    y2_2 = box2['y_center'] + box2['h'] / 2
    
    # Calculate intersection
    xi1 = max(x1_1, x1_2)
    yi1 = max(y1_1, y1_2)
    xi2 = min(x2_1, x2_2)
    yi2 = min(y2_1, y2_2)
    
    if xi2 <= xi1 or yi2 <= yi1:
        return 0.0
    
    inter_area = (xi2 - xi1) * (yi2 - yi1)
    
    # Calculate union
    area1 = box1['w'] * box1['h']
    area2 = box2['w'] * box2['h']
    union_area = area1 + area2 - inter_area
    
    if union_area <= 0:
        return 0.0
    
    return inter_area / union_area


# ==================== RECENT PROJECTS ====================

def get_recent_projects(session_path="session.json", max_projects=10):
    """Get list of recent projects from session file."""
    session = load_session(session_path)
    return session.get('recent_projects', [])


def add_recent_project(name, image_dir, output_dir, session_path="session.json", max_projects=10):
    """Add or update a project in the recent projects list."""
    session = load_session(session_path)
    recent = session.get('recent_projects', [])
    
    # Remove existing entry with same directories
    recent = [p for p in recent if p.get('image_dir') != image_dir]
    
    # Add new entry at the beginning
    recent.insert(0, {
        'name': name,
        'image_dir': image_dir,
        'output_dir': output_dir
    })
    
    # Trim to max size
    recent = recent[:max_projects]
    
    session['recent_projects'] = recent
    save_session(session_path, session)
