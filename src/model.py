import os
import tkinter as tk
from src.utils import (load_classes, load_config,
                       load_session, save_session, get_annotated_images,
                       parse_yolo, save_yolo, ImagePreloader, validate_annotations,
                       get_recent_projects, add_recent_project,
                       SESSION_FILE, CONFIG_FILE, CLASSES_FILE)

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
        self.show_filmstrip = tk.BooleanVar(value=self.session.get('show_filmstrip', True))
        self.unannotated_filter_active = tk.BooleanVar(value=False)
        self.current_theme_name = self.session.get('theme_name', 'Midnight Glass')
        
        # Annotation State
        self.boxes = []
        self.selected_indices = set()
        self.clipboard = []
        self.preview_boxes = []
        self.preview_visible = False
        self.preview_source = None
        self.preview_slot = None
        self.current_class_index = -1
        
        # Filter State
        self.filtered_classes = [(i, c) for i, c in enumerate(self.classes)]
        self.class_counts = {}
        
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
        
        # History for Undo/Redo
        self.history = []
        self.redo_stack = []
        self.max_history = 50
        
        # Bounding Box Presets (Slots 1-9)
        self.presets = {}
        self.load_scoped_presets()
        
        # Image Preloader for faster navigation
        self.image_preloader = ImagePreloader(cache_size=7)
        
        # Recent Projects
        self.recent_projects = get_recent_projects(SESSION_FILE)

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
        self._store_presets_for_current_scope()
        session_state = {
            "last_image_dir": self.image_dir,
            "last_output_dir": self.output_dir,
            "last_image_index": self.current_image_index,
            "zoom_factor": self.zoom_factor,
            "show_labels": self.show_labels.get(),
            "auto_save": self.auto_save.get(),
            "show_right_sidebar": self.show_right_sidebar.get(),
            "show_left_sidebar": self.show_left_sidebar.get(),
            "show_filmstrip": self.show_filmstrip.get(),
            "theme_name": self.current_theme_name,
            "box_presets_by_scope": self.session.get('box_presets_by_scope', {})
        }
        self.session.update(session_state)
        save_session(SESSION_FILE, session_state)

    def get_preset_scope_key(self):
        image_dir = os.path.abspath(self.image_dir) if self.image_dir else "__no_image_dir__"
        output_dir = os.path.abspath(self.output_dir) if self.output_dir else "__no_output_dir__"
        class_names = [c['name'] for c in self.classes]
        class_signature = "||".join(class_names) if class_names else "__no_classes__"
        return f"{image_dir}::{output_dir}::{class_signature}"

    def load_scoped_presets(self):
        scope_key = self.get_preset_scope_key()
        preset_store = self.session.get('box_presets_by_scope', {})
        scoped_presets = preset_store.get(scope_key, {})
        self.presets = {
            int(slot): [dict(box) for box in boxes]
            for slot, boxes in scoped_presets.items()
            if str(slot).isdigit()
        }

    def _store_presets_for_current_scope(self):
        scope_key = self.get_preset_scope_key()
        preset_store = self.session.setdefault('box_presets_by_scope', {})

        if self.presets:
            preset_store[scope_key] = {
                str(slot): [dict(box) for box in boxes]
                for slot, boxes in sorted(self.presets.items())
                if boxes
            }
        else:
            preset_store.pop(scope_key, None)

    def set_preview(self, boxes, source, slot=None):
        self.preview_boxes = [dict(box) for box in boxes]
        self.preview_visible = bool(self.preview_boxes)
        self.preview_source = source if self.preview_visible else None
        self.preview_slot = slot if self.preview_visible else None

    def clear_preview(self):
        self.preview_boxes = []
        self.preview_visible = False
        self.preview_source = None
        self.preview_slot = None

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

    def validate_current_annotations(self):
        """Validate current image annotations and return warnings."""
        return validate_annotations(self.boxes, self.classes)
    
    def add_to_recent_projects(self, name=None):
        """Add current project to recent projects list."""
        if not self.image_dir:
            return
        
        # Use directory name as project name if not provided
        if not name:
            name = os.path.basename(self.image_dir.rstrip(os.sep))
        
        add_recent_project(name, self.image_dir, self.output_dir, SESSION_FILE)
        self.recent_projects = get_recent_projects(SESSION_FILE)
