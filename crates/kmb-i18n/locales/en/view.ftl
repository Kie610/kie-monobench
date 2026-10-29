### Items drawn over the 3D view (top-left hint, ISO and full-screen buttons, bottom bar, display menu,
### browser grip, failed-recompute banner, machining check legend). Feature and face names are document content and stay untranslated.

## Top-left hint

view-hint-select = Select
# $count: number
view-hint-vertices = { $count ->
    [one] 1 vertex
   *[other] { $count } vertices
}
view-hint-edges = { $count ->
    [one] 1 edge
   *[other] { $count } edges
}
view-hint-faces = { $count ->
    [one] 1 face
   *[other] { $count } faces
}
# Separator between the vertex, edge and face counts
view-hint-separator = { ", " }
# $parts: list of counts such as "2 vertices, 1 edge"
view-hint-mixed = { $parts } selected. Shift/Ctrl+click to add or remove
# $count: number of faces, $face: name of the last face picked
view-hint-faces-many = { $count } faces selected (last: { $face }). Shift/Ctrl+click to add or remove
view-hint-face-one = Selected face: { $face } (Shift/Ctrl+click to add)
# Heading while the Simulation workspace shows color-coded results
view-hint-result = Results
# Heading while Manufacture shows the in-process stock shape
view-hint-stage-shape = In-process stock

## Top-right buttons

view-fullscreen = Full screen
view-fullscreen-exit = Exit full screen
view-iso = ISO
# Name while pressed again to look from the opposite side
view-iso-back = ISO (back)

## Bottom bar

view-orbit = Orbit
view-pan = Pan
view-zoom = Zoom
# $how: current orbit mode (view-orbit-constrained or view-orbit-free)
view-orbit-tip = While on, left-drag orbits (press again or Esc to return). Now: { $how }. Right-click to change
view-pan-tip = While on, left-drag pans (press again or Esc to return)
view-zoom-tip = While on, left-drag zooms (press again or Esc to return)
view-orbit-constrained = Constrained orbit
view-orbit-free = Free orbit
view-orbit-constrained-item = Constrained orbit (keep up direction)
view-fit = Fit
view-fit-tip = Fit everything in view and orbit around the center of the part (Home)
view-look-at = Look At
view-look-at-tip = Look straight at the selected face, sketch or origin plane and fit the whole shape
# $why: why it cannot be used (view-look-at-no-target etc.)
view-look-at-why = Look straight at the selected face, sketch or origin plane. { $why }
view-look-at-no-target = Select a face, sketch or origin plane first
view-look-at-no-normal = The direction of this face is unknown
view-display = Display ▾
view-display-tip = Browser, timeline, edit panel, visual style and projection, grid and origin

## Display menu

view-menu-panels = Panels
view-menu-data-panel = Data panel
view-menu-data-panel-tip = Recent and project designs (same as the leftmost button in the top bar)
view-menu-browser = Browser
view-menu-timeline = Timeline (operation bar in Manufacture)
view-menu-timeline-off = Not shown in Render and Simulation (shown again in Design)
view-menu-panel-always = Show the edit panel on selection
view-menu-panel-always-tip = Off: the edit panel appears only while creating, editing or in Parameters. On: selecting a feature shows its values
view-menu-commands = Commands
view-menu-commands-tip = Command line: press or type into UI items by name, run scripts (type help for a list)
view-menu-reset-dock = Reset layout
view-menu-reset-dock-tip = Put panels you dragged by their tabs back where they started (and show hidden ones)

## Visual style, projection, guides and effects (display menu, right-click on empty space, settings)

view-style = Visual style
view-shading-shaded-edges = Shaded with visible edges only
view-shading-shaded-hidden = Shaded with hidden edges
view-shading-shaded = Shaded
view-shading-wireframe = Wireframe
view-projection = Projection
view-projection-parallel = Orthographic
view-projection-parallel-tip = No perspective; parallel lines stay parallel
view-projection-perspective = Perspective
view-projection-perspective-tip = Farther objects look smaller
view-projection-auto = Auto
view-projection-auto-tip = Orthographic when looking along the X, Y or Z axis (top, front, right…), perspective otherwise
view-guides = Guides
view-grid = Grid
view-grid-tip = Grid on the XY plane. Spacing follows the zoom (current spacing at the bottom left)
view-origin = Origin and axes
view-origin-tip = Origin and the X (red), Y (green) and Z (blue) axes. Base planes too in an empty document
view-see-through = See through all
view-see-through-tip = Show sketches and construction planes behind bodies with normal lines. When off, hidden parts are faint dotted lines
view-effects = Effects
view-materials = Material appearance
view-materials-tip = Draw bodies that have an appearance or physical material with that material's look (wood grain, metal reflections, gloss). The Render workspace always draws every body this way
view-env-studio = Studio (light)
view-env-studio-dark = Studio (dark)
view-env-outdoor = Outdoor

## Grip at the left edge while the browser is hidden

# Drawn one character per line; keep it short
view-grip = Browser
view-grip-tip = Show the browser

## Notices and legends over the view

# $names: names of the features that failed ("Extrude1, Fillet2")
view-held = { $names } failed to compute, so the last good shape is shown
# $count: number of marks
view-mark-gouge = { $count ->
    [one] Red dot: gouge or tool collision at 1 place
   *[other] Red dots: gouges or tool collisions at { $count } places
}
view-mark-leftover = { $count ->
    [one] Purple dot: leftover stock at 1 place (larger = thicker)
   *[other] Purple dots: leftover stock at { $count } places (larger = thicker)
}

## Overlapping candidates (long press)

view-candidates = Overlapping items (front to back)

view-part-hues = Color by Component
view-part-hues-tip = Distinguish bodies without appearances or materials with a color for each component. Remember this view per document.
