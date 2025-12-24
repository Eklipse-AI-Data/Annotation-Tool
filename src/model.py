import os
import tkinter as tk
from src.utils import (load_classes, load_config, save_config, 
                       load_session, save_session, get_annotated_images,
                       parse_yolo, save_yolo)

SESSION_FILE = "session.json"
CONFIG_FILE = "config.json"
CLASSES_FILE = "data/predefined_classes.txt"

class AnnotationModel:
    def __init__(self):
        # Load persistent data
        self.session = load_session(SESSION_FILE)
        self.config = load_config(CONFIG_FILE)
        self.classes = load_classes(CLASSES_FILE)
        
        # Application State
        self.image_dir = self.session.get('last_image_dir', '')
        self.output_dir = self.session.get('last_output_dir', '')
        self.image_list = []
        self.full_image_list = []
        self.current_image_index = -1
        self.current_image_pil = None
        
        # UI/View State (to be synced)
        self.zoom_factor = self.session.get('zoom_factor', 1.0)
        self.auto_save = tk.BooleanVar(value=self.session.get('auto_save', True))
        self.show_labels = tk.BooleanVar(value=self.session.get('show_labels', True))
        self.show_right_sidebar = tk.BooleanVar(value=self.session.get('show_right_sidebar', True))
        self.show_left_sidebar = tk.BooleanVar(value=self.session.get('show_left_sidebar', True))
        self.unannotated_filter_active = tk.BooleanVar(value=False)
        self.current_theme_name = self.session.get('theme_name', 'Midnight Glass')
        
        # Annotation State
        self.boxes = []
        self.selected_indices = set()
        self.clipboard = []
        self.current_class_index = -1
        
        # Filter State
        self.filtered_classes = [(i, c) for i, c in enumerate(self.classes)]
        
        # Interaction State
        self.is_drawing = False
        self.resize_mode = False
        self.move_mode = False
        self.is_panning = False
        self.last_x = 0
        self.last_y = 0
        self.rect_id = None
        self.start_x = 0
        self.start_y = 0
        self.active_handle_index = -1
        self.active_box_index = -1
        self.pan_start_x = 0
        self.pan_start_y = 0
        self.template_mode = False
        
        # History for Undo/Redo
        self.history = []
        self.redo_stack = []
        self.max_history = 50
        
        # Bounding Box Presets (Slots 1-9)
        self.presets = {}

    def save_state(self):
        """Saves current boxes to history stack."""
        # Only save if different from last state
        current_state = [dict(b) for b in self.boxes]
        if not self.history or self.history[-1] != current_state:
            self.history.append(current_state)
            if len(self.history) > self.max_history:
                self.history.pop(0)
            self.redo_stack.clear()

    def undo(self):
        """Restores boxes from history stack."""
        if not self.history:
            return False
            
        # Current state goes to redo stack
        self.redo_stack.append([dict(b) for b in self.boxes])
        
        # Restore last state
        self.boxes = self.history.pop()
        self.selected_indices.clear()
        return True

    def redo(self):
        """Restores boxes from redo stack."""
        if not self.redo_stack:
            return False
            
        # Current state goes to history
        self.history.append([dict(b) for b in self.boxes])
        
        # Restore from redo
        self.boxes = self.redo_stack.pop()
        self.selected_indices.clear()
        return True

    def save_session(self):
        session_state = {
            "last_image_dir": self.image_dir,
            "last_output_dir": self.output_dir,
            "last_image_index": self.current_image_index,
            "zoom_factor": self.zoom_factor,
            "show_labels": self.show_labels.get(),
            "auto_save": self.auto_save.get(),
            "show_right_sidebar": self.show_right_sidebar.get(),
            "show_left_sidebar": self.show_left_sidebar.get(),
            "theme_name": self.current_theme_name
        }
        save_session(SESSION_FILE, session_state)

    def load_annotations(self, filename):
        self.boxes = []
        self.selected_indices = set()
        if not self.output_dir or not filename:
            return
            
        name, _ = os.path.splitext(filename)
        txt_path = os.path.join(self.output_dir, name + ".txt")
        
        if os.path.exists(txt_path):
            self.boxes = parse_yolo(txt_path, 0, 0)

    def save_annotations(self):
        if self.current_image_index == -1 or not self.output_dir or not self.image_list:
            return
            
        filename = self.image_list[self.current_image_index]
        name, _ = os.path.splitext(filename)
        txt_path = os.path.join(self.output_dir, name + ".txt")
        
        # Filter boxes to ensure valid classes
        valid_boxes = [b for b in self.boxes if any(c['id'] == b['class_id'] for c in self.classes) or b['class_id'] == -1]
        
        if valid_boxes or os.path.exists(txt_path):
             final_boxes = [b for b in valid_boxes if b['class_id'] != -1]
             save_yolo(txt_path, final_boxes)

    def get_annotated_count(self):
        if not self.full_image_list:
            return 0, 0
        annotated = get_annotated_images(self.image_dir, self.output_dir, self.full_image_list)
        return len(annotated), len(self.full_image_list)
