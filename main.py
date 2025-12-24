import tkinter as tk
from src.model import AnnotationModel
from src.view import AnnotationView
from src.controller import AnnotationController

def main():
    root = tk.Tk()
    
    model = AnnotationModel()
    view = AnnotationView(root, model)
    controller = AnnotationController(model, view)
    
    root.mainloop()

if __name__ == "__main__":
    main()
