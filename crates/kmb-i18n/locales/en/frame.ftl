### Window frame (window buttons, top bar, notifications, document tabs, status bar)

## Window buttons (right end of the title bar drawn by the app)

frame-window-minimize = Minimize
frame-window-restore = Restore Down
frame-window-maximize = Maximize
frame-window-close = Close

## Top bar

frame-data-panel = Data Panel
frame-data-panel-hide = Hide Data Panel
frame-data-panel-show = Show Data Panel: lists recent and project designs on the left; open them, or drag one into the 3D view to insert it as a link
frame-save = Save
frame-save-tip = Save (Ctrl+S)
frame-undo = Undo
frame-undo-tip = Undo (Ctrl+Z)
frame-redo = Redo
frame-redo-tip = Redo (Ctrl+Y)
frame-undo-history = Undo History
frame-undo-history-tip = Undo History: the changes made so far. Click a row to go back to that point
frame-settings = Preferences
frame-settings-tip = Preferences: mouse, layout, machines and other settings for this PC
frame-help = Help
# Faint text inside the button shaped like a search box
frame-search-placeholder = Search commands
frame-search = Command Search
frame-search-tip = Find a command by name and run it (Ctrl+K)

## Notifications (bell button and list)

# $count: number of notifications
frame-notices = Notifications ({ $count })
frame-notices-tip = Notifications: calculation warnings, saves and export results
# $count: number of notifications
frame-notices-heading = { $count ->
    [one] Notifications · 1 item
   *[other] Notifications · { $count } items
}
frame-notices-empty = No notifications
frame-notices-copy = Copy
frame-notices-copy-tip = Copy this notification to the clipboard
frame-notices-copy-all = Copy All
frame-notices-copy-all-tip = Copy all notifications, oldest first, with times, to the clipboard
frame-notices-clear = Clear History

## Document tabs

frame-tab-untitled = (Untitled)
# $path: location of the document file
frame-tab-saved = { $path }
# $path: location of the document file
frame-tab-saved-modified = { $path } (unsaved changes)
frame-tab-unsaved = Document not saved yet
frame-tab-unsaved-modified = Document not saved yet (unsaved changes)
frame-tab-close = Close
frame-tab-close-others = Close Other Tabs
frame-tab-more = More Tabs
# $count: number of tabs that do not fit in the bar
frame-tab-more-tip = { $count ->
    [one] 1 tab does not fit in the bar (click for the list)
   *[other] { $count } tabs do not fit in the bar (click for the list)
}
frame-tab-new = New Tab
frame-tab-new-tip = Open a new design in another tab (Ctrl+N)

## Status bar

# $unit: name of the length unit (mm, in, ...)
frame-status-unit = Units { $unit }
frame-status-unit-tip = Length unit of this document. Click to change it in Preferences > Units
frame-status-recalculating = Recalculating
# $secs: seconds elapsed
frame-status-recalculating-secs = Recalculating { $secs } s
frame-status-recalculating-tip = You can still orbit and select while it calculates. The shape is replaced when it finishes
frame-status-computing = Computing
# $secs: seconds elapsed
frame-status-computing-secs = Computing { $secs } s
frame-status-computing-tip = You can still orbit and select while it calculates. You will be notified when it finishes
# $note: job name and progress, $elapsed: time elapsed (e.g. 1:05)
frame-status-progress = { $note } · Elapsed { $elapsed }
# $note: job name and progress, $elapsed: time elapsed, $left: estimated time left
frame-status-progress-left = { $note } · Elapsed { $elapsed } · About { $left } left
frame-status-finish-tip = Finish the image with the passes so far and write the file (may still be noisy)
frame-status-stop = Stop
frame-status-stop-tip = Stop here. No file is written
frame-status-dismiss-tip = Dismiss this notification (it stays in the bell list)
# $count: number of selected items
frame-status-selected = { $count ->
    [one] 1 item selected
   *[other] { $count } items selected
}
frame-status-files-local = Files are saved on this PC. No internet connection is needed.

## Status bar hints (left: key or button name, right: what it does)

frame-key-left = Left
frame-key-right = Right
frame-key-digits = Digits
frame-key-middle = Middle
frame-key-wheel = Wheel
frame-hint-ok = OK
frame-hint-click-to-set = Click to place
frame-hint-type-value = Type a value
frame-hint-cancel = Cancel
frame-hint-group = Group
frame-hint-delete = Delete
frame-hint-select = Select · Drag for a box · Hold for overlaps
frame-hint-menu-orbit = Menu · Drag to orbit
frame-hint-middle-pan = Pan · Double-click to set the orbit center
frame-hint-middle-orbit = Orbit · Double-click to set the orbit center
frame-hint-zoom = Zoom
