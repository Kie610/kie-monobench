### Settings window

settings-window-title = Settings
settings-search-hint = Search settings (e.g. "font", "key", "projection")
# $query: the words typed, $count: number of matching pages
settings-search-found = { $count ->
    [one] 1 page matches "{ $query }"
   *[other] { $count } pages match "{ $query }"
}

## Tabs at the top

settings-tab-general = General
settings-tab-design = Design
settings-tab-machining = Manufacture

## Page names on the left

settings-page-appearance = Appearance
settings-page-input = Mouse and Keys
settings-page-files = Files
settings-page-render = Graphics
settings-page-units = Units
settings-page-new-doc = New Design
settings-page-arrangement = Layout
settings-page-ai = AI Connection
settings-page-view3d = 3D View

## Appearance page: display language

settings-language = Display language
# $lang: name of the OS language, written in that language (e.g. 日本語)
settings-language-auto = Automatic (follow the OS: { $lang })
settings-language-hint = Language of the on-screen text. Text that has not been translated yet is shown in English (Simplified Chinese, Traditional Chinese and Korean are open for translation)

## Input display

settings-keycast-title = Input Display
settings-keycast-enabled = Show keys and mouse buttons on screen (for recordings and tutorials)
settings-keycast-contents = Show
settings-keycast-keys = Keys
settings-keycast-mouse = Mouse
settings-keycast-mouse3d = 3D mouse
settings-keycast-names = Show command names
settings-keycast-place = Position
settings-keycast-left = Bottom left
settings-keycast-center = Bottom center
settings-keycast-scale = Size
settings-keycast-hold = Time before hiding
settings-keycast-seconds =  s
settings-keycast-private = While typing in a text field, only Ctrl or Alt combinations and Enter, Esc and Tab are shown

## Fixed names of keyboard commands

settings-key-name-search-here = Search commands at the pointer (by name or reading)
settings-key-name-search = Search commands at the top of the screen
settings-key-name-extrude = Start extrude (extrude the selected regions while editing a sketch)
settings-key-name-hole = Start hole
settings-key-name-press-pull = Press/Pull (extrude a face, round an edge, or edit its feature)
settings-key-name-measure = Measure
settings-key-name-appearance = Open appearance (color and texture for bodies, components and faces)
settings-key-name-move = Start move/copy
settings-key-name-toggle-visible = Hide/show selected bodies or features
settings-key-name-redo = Redo
settings-key-name-redo-alt = Redo
settings-key-name-undo = Undo
settings-key-name-save-as = Save As
settings-key-name-save = Save
settings-key-name-new = New Design
settings-key-name-open = Open
settings-key-name-close-tab = Close the current document tab
settings-key-name-next-tab = Next document tab
settings-key-name-prev-tab = Previous document tab
settings-key-name-recompute = Recompute everything
settings-key-name-group = Group selected features in the timeline
settings-key-name-params = Show/hide the parameters list
settings-key-name-settings = Open/close settings
settings-key-name-sketch-line = Sketch: Line
settings-key-name-sketch-rect = Sketch: Rectangle
settings-key-name-sketch-circle = Sketch: Circle
settings-key-name-sketch-point = Sketch: Point
settings-key-name-sketch-trim = Sketch: Trim
settings-key-name-sketch-offset = Sketch: Offset
settings-key-name-sketch-dimension = Sketch: Dimension (dimension selected lines in a 3D sketch)
settings-key-name-sketch-construction = Sketch: Toggle construction geometry; while drawing, use construction geometry for the next line
