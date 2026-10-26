### Timeline (the row of features at the bottom: status marks and tooltips, rollback marker, groups, feature right-click)
### Feature and group names belong to the document and are not translated. Warning and error messages themselves belong to the messages area.

## Calculation status

timeline-health-warning = Warning
timeline-health-error = Error
timeline-health-blocked = Not calculated
timeline-health-ok = Calculated
# $by: name of the earlier feature that stops this one
timeline-health-blocked-by = This feature was not calculated because { $by } failed
timeline-status-blocked = { $by } failed
timeline-status-suppressed = Suppressed
timeline-status-rolled-back = After the rollback marker

## Feature tooltips ($name: feature name, $title: kind of status, $message: the message)

timeline-tip-error =
    { $name }
    { $title }: { $message }
    Right-click "Review Error" for details, or "Copy Message" to copy it
timeline-tip-warning =
    { $name }
    { $title }: { $message }
    Right-click "Review Warning" for details, or "Copy Message" to copy it
# $value: seconds with one decimal
timeline-seconds = { $value } s
# $time: time taken ("12 ms", "1.5 s")
timeline-calc-time = Calculated in { $time }
timeline-calc-reused = { " " }(reused the previous result)
timeline-chip-hint = Double-click to edit, drag to reorder, right-click for more

## Row and status line

timeline-empty = No features yet. Create one from the toolbar above
timeline-rollback-start = Move the rollback marker to the start
timeline-rollback-back = Move the rollback marker back one step (temporarily turns off the features after it)
timeline-rollback-forward = Move the rollback marker forward one step
timeline-rollback-end = Move the rollback marker to the end (turns everything on)
timeline-marker = Rollback Marker
timeline-count = { $count ->
    [one] 1 feature
   *[other] { $count } features
}
timeline-count-errors = { $count ->
    [one] 1 error
   *[other] { $count } errors
}
timeline-count-warnings = { $count ->
    [one] 1 warning
   *[other] { $count } warnings
}
timeline-count-groups = { $count ->
    [one] 1 group
   *[other] { $count } groups
}
timeline-count-rolled = { $count } after the rollback marker
# While dragging two or more features
timeline-drag-many = { $count } features
timeline-inside-warning = Contains a warning (expand to see)
timeline-inside-error = Contains an error (expand to see)
timeline-inside-blocked = Contains features that were not calculated (expand to see)
timeline-find-in-browser = Find in Browser
timeline-find-in-browser-tip = Highlight this feature's body in the browser

## Groups ($name: group name)

timeline-group-expand = Expand group "{ $name }"
timeline-group-collapse = Collapse group "{ $name }"
timeline-group-expand-tip =
    Group "{ $name }" ({ $count })
    Click to expand. Right-click to rename, ungroup or delete
timeline-group-collapse-tip =
    Group "{ $name }" ({ $count })
    Click to collapse. Right-click to rename, ungroup or delete
timeline-group-name = Group Name
timeline-group-select-tip = Click to select everything in it. Right-click to rename, ungroup or delete
timeline-group-cannot = Cannot Group
timeline-group-include-gaps = Include Features Between
timeline-group-include-gaps-tip = Also include the features between the first and the last one
timeline-group-gather = Gather, Then Group
timeline-group-gather-tip = Move the features in between to after the group and group only the selected ones
timeline-group-rename = Rename
timeline-group-open = Expand
timeline-group-close = Collapse
timeline-group-suppress = Suppress All
timeline-group-unsuppress = Unsuppress All
timeline-ungroup = Ungroup (Keep Contents)
timeline-group-delete = Delete Group and Contents
timeline-group-delete-tip = Also deletes the { $count } features inside. Undo (Ctrl+Z) brings them back

## Feature right-click (same in the timeline, browser and 3D view)

timeline-menu-review-error = Review Error
timeline-menu-review-warning = Review Warning
timeline-menu-review-tip = Show the full message and hints for fixing it at the bottom right of the view
timeline-menu-copy = Copy Message
timeline-menu-copy-tip = Copy the feature name and the message to the clipboard (to paste in a question or report)
timeline-menu-select-cause = Select Cause Feature
timeline-menu-select-cause-tip = Select the earlier feature that stops this one
timeline-menu-edit = Edit Feature
timeline-menu-edit-tip = Edit this feature in the edit panel (double-click also opens it)
timeline-menu-edit-many = { $count ->
    [one] Edit 1 Together
   *[other] Edit { $count } Together
}
timeline-menu-edit-many-tip = Edit the shared fields of all selected features at once (works when they are the same kind)
timeline-menu-edit-sketch = Edit Sketch
# $name: sketch name
timeline-menu-edit-sketch-of = Edit Source Sketch "{ $name }"
timeline-menu-edit-sketch-tip = Open and edit the sketch "{ $name }" used by this feature
timeline-menu-rename = Rename
timeline-menu-rename-tip = Change the name shown in the timeline and browser (can be undone)
timeline-menu-suppress = Suppress
timeline-menu-unsuppress = Unsuppress
timeline-menu-roll-back = Roll Back to Here
timeline-menu-roll-back-tip = Temporarily turn off the features after this one (they are not deleted)
timeline-menu-roll-forward = Roll Forward to End
timeline-menu-move-up = Move Up
timeline-menu-move-down = Move Down
timeline-menu-group = Group ({ $count })
# $name: group name
timeline-menu-take-out = Remove from "{ $name }"
timeline-menu-copy-feature = Copy
timeline-menu-copy-feature-tip = Copy the values and expressions. Faces and edges are picked again when pasting
timeline-menu-paste-feature = Paste
timeline-menu-paste-feature-tip = Paste at the end of the timeline and pick the faces and edges again
timeline-menu-paste-feature-empty = Copy a feature first (Ctrl+C)
timeline-menu-delete = Delete
timeline-menu-delete-tip = Undo (Ctrl+Z) brings it back
