import tkinter as tk
from tkinter import ttk

# Eclipse-like Dark Theme Colors
THEME = {
    'bg_main': '#252526',       # VS Code / Eclipse Dark
    'bg_sidebar': '#1e1e1e',    # Slightly darker
    'fg_text': '#cccccc',       # Light grey text
    'fg_highlight': '#ffffff',  # White text
    'accent': '#007acc',        # Blue accent
    'selection': '#264f78',     # Selection blue
    'border': '#3e3e42',        # Dark border
    'button_bg': '#3c3c3c',
    'button_fg': '#cccccc',
    'button_hover': '#505050',
    'entry_bg': '#3c3c3c',
    'entry_fg': '#cccccc',
}

class DarkFrame(tk.Frame):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_main'])
        super().__init__(master, **kwargs)

class SidebarFrame(tk.Frame):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_sidebar'])
        super().__init__(master, **kwargs)

class DarkLabel(tk.Label):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_main'])
        kwargs.setdefault('fg', THEME['fg_text'])
        kwargs.setdefault('font', ('Segoe UI', 10))
        super().__init__(master, **kwargs)

class SectionLabel(tk.Label):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_sidebar'])
        kwargs.setdefault('fg', THEME['fg_highlight'])
        kwargs.setdefault('font', ('Segoe UI', 11, 'bold'))
        kwargs.setdefault('pady', 5)
        super().__init__(master, **kwargs)

class DarkButton(tk.Button):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['button_bg'])
        kwargs.setdefault('fg', THEME['button_fg'])
        kwargs.setdefault('activebackground', THEME['button_hover'])
        kwargs.setdefault('activeforeground', THEME['fg_highlight'])
        kwargs.setdefault('relief', tk.FLAT)
        kwargs.setdefault('bd', 0)
        kwargs.setdefault('padx', 10)
        kwargs.setdefault('pady', 5)
        kwargs.setdefault('font', ('Segoe UI', 10))
        kwargs.setdefault('cursor', 'hand2')
        super().__init__(master, **kwargs)
        
        self.bind('<Enter>', self.on_enter)
        self.bind('<Leave>', self.on_leave)

    def on_enter(self, e):
        if self['state'] != 'disabled':
            self['bg'] = THEME['button_hover']

    def on_leave(self, e):
        if self['state'] != 'disabled':
            self['bg'] = THEME['button_bg']

class DarkEntry(tk.Entry):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['entry_bg'])
        kwargs.setdefault('fg', THEME['entry_fg'])
        kwargs.setdefault('insertbackground', THEME['fg_text']) # Cursor color
        kwargs.setdefault('relief', tk.FLAT)
        kwargs.setdefault('bd', 5) # Padding inside
        kwargs.setdefault('font', ('Segoe UI', 10))
        super().__init__(master, **kwargs)

class DarkListbox(tk.Listbox):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['entry_bg'])
        kwargs.setdefault('fg', THEME['entry_fg'])
        kwargs.setdefault('selectbackground', '#FFFFFF') # White selection
        kwargs.setdefault('selectforeground', '#000000') # Black text
        kwargs.setdefault('relief', tk.FLAT)
        kwargs.setdefault('bd', 0)
        kwargs.setdefault('highlightthickness', 0)
        kwargs.setdefault('font', ('Segoe UI', 10))
        kwargs.setdefault('activestyle', 'none')
        super().__init__(master, **kwargs)

class DarkScrollbar(ttk.Scrollbar):
    def __init__(self, master, **kwargs):
        # Styling scrollbars in Tkinter is hard, using default ttk for now
        # but could try to theme it if needed.
        super().__init__(master, **kwargs)
