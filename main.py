import tkinter as tk
from src.app import AnnotationApp

def main():
    root = tk.Tk()
    # Set icon if available (optional)
    # root.iconbitmap('icon.ico') 
    
    app = AnnotationApp(root)
    root.mainloop()

if __name__ == "__main__":
    main()
