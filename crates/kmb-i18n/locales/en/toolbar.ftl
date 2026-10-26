### Toolbar (workspace box, tabs, groups, buttons owned by the screen, selection group)
### Command names are in action.ftl, names of features added with fields in feature.ftl.

## Workspace box

toolbar-space = Workspace
toolbar-space-design = Design
toolbar-space-machining = Manufacture
toolbar-space-render = Render
toolbar-space-animation = Animation
toolbar-space-simulation = Simulation
# $name: name of the current workspace
toolbar-space-now = Workspace: { $name }
toolbar-space-tip = Workspace: Design (make shapes) and Manufacture (make toolpaths)

## Tabs

toolbar-tab-solid = Solid
toolbar-tab-sketch = Sketch
toolbar-tab-render = Render
toolbar-tab-animation = Animation
toolbar-tab-milling = Milling
toolbar-tab-simulation = Simulation
toolbar-move-left = Move Left
toolbar-move-right = Move Right
toolbar-tab-order-reset = Reset Tab Order

## Groups (names under the band)

toolbar-group-create = Create
toolbar-group-modify = Modify
toolbar-group-construct = Construct
toolbar-group-pattern = Pattern
toolbar-group-assemble = Assemble
toolbar-group-inspect = Inspect
toolbar-group-insert = Insert
toolbar-group-export = Export
# Group in the Render workspace
toolbar-group-setup = Setup
toolbar-group-canvas-render = In-canvas Render
toolbar-group-render = Render
toolbar-group-transform = Transform
toolbar-group-view = View
toolbar-group-publish = Publish
# Group in the Manufacture workspace (stock and origin)
toolbar-group-cam-setup = Setup
toolbar-group-2d = 2D
toolbar-group-3d = 3D
toolbar-group-drilling = Drilling
toolbar-group-check = Inspect
toolbar-group-nc-program = NC Program
toolbar-group-output = Output
toolbar-group-manage = Manage
toolbar-group-study = Study
toolbar-group-flow = Flow
toolbar-group-solve = Solve
toolbar-group-results = Results
toolbar-group-constraints = Constraints
toolbar-group-dimension = Dimension
# Group of tools that make solids from a sketch
toolbar-group-solid = Solid
toolbar-group-select = Select
toolbar-group-select-tip = What can be selected. With Body on, clicking a face selects its whole body
toolbar-group-more = More
toolbar-group-more-tip = Groups that do not fit on the band. Widen the window, or turn off button names in the settings, to show them on the band
# $name: name of the group
toolbar-collapsed = { $name } (collapsed group)
# $name: name of the group
toolbar-collapsed-tip = { $name }: a group collapsed into one button because the window is narrow. Press it for the list of tools

## Buttons in front (pins)

toolbar-pin = Pin to Front
toolbar-unpin = Remove from Front
toolbar-pins-reset = Reset Front Buttons
toolbar-group-reset = Reset This Group

## Buttons owned by the screen

toolbar-measure = Measure
toolbar-measure-tip = M. Click points, edges and faces to measure distance, angle, diameter and area
toolbar-section = Section Analysis
toolbar-section-tip = Cut with a plane to see inside (the shape is unchanged). Select a flat face first to cut parallel to it
toolbar-press-pull = Press Pull
toolbar-press-pull-tip = Acts on what is selected: sketch profiles and flat faces are extruded, edges are filleted, and fillet, chamfer and hole faces edit their feature
toolbar-scene = Scene Settings
toolbar-scene-tip = Environment light (studio, outdoor) with brightness and rotation, background (environment, solid color, transparent), ground plane, camera focal length, exposure and depth of field, aspect ratio, image and video size and length. "Rendering" in the settings
toolbar-appearance = Appearance
toolbar-appearance-tip = A. Open the window to give bodies, components and faces an appearance (color, metal, roughness). Search the library by category and name, and edit appearances in this design
toolbar-canvas-render = In-canvas Render
toolbar-canvas-render-tip = Ray-trace the current view and refine it while the view is still (shadows and reflections). Press again to stop
toolbar-params = Parameters
toolbar-params-tip = Ctrl+P. Show the parameter list in the edit panel on the right
toolbar-tools = Tool Library
toolbar-tools-tip = Select the tool list (adds one if there is none)
toolbar-machine = Machine Settings
toolbar-machine-tip = Machine values (work area, spindle speed, collet, bed). "Machine" in the settings. A machine library with several machines is not available yet
# $name: name of the feature, to tell it apart from the click-to-create command of the same name
toolbar-feature-by-fields = { $name } (by Fields)
# $key: assigned key (such as Ctrl+P; not translated)
toolbar-key = Key: { $key }
toolbar-key-not-here = This key cannot be used in this workspace

## Selection group

toolbar-pick-vertex = Vertex
toolbar-pick-edge = Edge
toolbar-pick-face = Face
toolbar-pick-body = Body
toolbar-pick-vertex-check = Select Vertices
toolbar-pick-edge-check = Select Edges
toolbar-pick-face-check = Select Faces
toolbar-pick-body-check = Select Bodies (clicking a face selects its body)
# $name: name of the field waiting for a selection
toolbar-pick-field = What the field "{ $name }" accepts
toolbar-pick-now-body = Clicking a face selects its whole body
toolbar-pick-now-mixed = Select vertices, edges and faces together
toolbar-pick-all = Allow Selecting Everything
