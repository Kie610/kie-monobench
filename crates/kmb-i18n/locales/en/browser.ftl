### Browser (the tree on the left: design, machining and simulation trees, kind chips, row eyes and radio buttons, right-click)
### Folder names in the tree are also keys for remembering what is open, so only the shown text is translated.

browser-title = Browser
browser-hide = Hide Browser
browser-hide-tip = Hide the browser (bring it back with "Browser" on the left edge)
browser-search = Search by name

## Kind chips

browser-kind-all = All
browser-kind-sketch = Sketches
browser-kind-body = Bodies
browser-kind-params = Parameters
browser-kind-operation = Operations
browser-kind-tools = Tools
browser-kind-other = Other

## Row eyes and radio buttons ($name: the row's name)

browser-eye-hide = Hide { $name }
browser-eye-show = Show { $name }
browser-eye-hide-tip = Hide in the 3D view (still used for calculations and machining)
browser-eye-show-tip = Show in the 3D view
browser-active = { $name } is active
browser-activate = Activate { $name }
browser-active-tip = Active: new bodies and components go inside it
browser-activate-tip = Activate: put new bodies and components inside it and dim the other components

## Design tree

# The document row when the document has no name yet
browser-untitled = (Untitled)
browser-root-activate = Activate
browser-root-activate-tip = Make the top level active again (new bodies are not put in a component)
browser-properties = Design Properties…
browser-origin = Origin
browser-plane-xy = XY Plane
browser-plane-yz = YZ Plane
browser-plane-zx = ZX Plane
browser-axis-x = X Axis
browser-axis-y = Y Axis
browser-axis-z = Z Axis
browser-named-views = Named Views
browser-save-view = Save Current View
browser-motion-studies = Motion Studies
browser-contact-sets = Contact Sets
# $names: body names joined with "・"
browser-contact-bodies = Bodies that touch: { $names }
browser-contact-delete = Delete
browser-params = Parameters
browser-params-tip = Ctrl+P. Show the list in the edit panel on the right
browser-folder-sketch = Sketches
browser-folder-body = Bodies
browser-folder-other = Other Features
browser-cam-elsewhere = { $count ->
    [one] 1 machining operation is shown in the Machining workspace
   *[other] { $count } machining operations are shown in the Machining workspace
}
browser-expand-all = Expand All
browser-expand-all-tip = Expand this row and every branch below it
browser-collapse-all = Collapse All
browser-collapse-all-tip = Collapse every branch below (this row stays open, showing only the rows right under it)
# When a collapsed folder holds a warning or an error
browser-inside-warning = Contains a warning (expand to see)
browser-inside-error = Contains an error (expand to see)
browser-inside-blocked = Contains items that were not calculated (expand to see)

## Body row right-click

browser-body-rename = Rename
browser-body-rename-tip = Change the body name shown in the browser (can be undone; double-clicking the row also works)
browser-body-show = Show
browser-body-hide = Hide
browser-body-visible-tip = Show or hide in the 3D view (hidden bodies are not exported)
browser-body-isolate = Isolate
browser-body-isolate-tip = Show only this body (and the other selected bodies). Undo it with "Unisolate" on the tag at the top of the 3D view
browser-body-unisolate = Unisolate
browser-body-unisolate-tip = Show the other bodies as before
browser-body-show-all = Show All Bodies
browser-body-show-all-tip = Stop isolating and show every hidden body
browser-body-param-view = Make a Parameter Table for This Body
browser-body-param-view-tip = Make a table in the parameters window with only the parameters, dimensions and feature values related to this body (can be undone)
browser-body-find-in-timeline = Find in Timeline
browser-body-find-in-timeline-tip = Highlight in the timeline the last feature that changed this body

## Simulation tree

browser-studies = Studies
browser-studies-empty = None yet. Add one with "Static Stress", "Thermal", "Modal Frequencies", "Lightweight Shape Guide", "Wing Lift", "2D Flow", "3D Flow" or "Sound Radiation and Scattering" above

## Machining tree

browser-machine = Machine
browser-machine-tip = Change the machine's values in Settings under "Machine"
browser-machine-settings = Machine Settings
browser-setups = Setups
browser-setups-none = No commands available
browser-new-setup = New Setup
browser-setups-empty = None yet. Add one with "Setup" above
browser-fixture = Fixtures
browser-clamps = { $count ->
    [one] 1 clamp
   *[other] { $count } clamps
}
browser-orphans = Operations Without a Setup
browser-tools = Tools in This Document
