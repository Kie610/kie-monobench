### The window that explains a feature's warning or error (bottom right of the view) and the count badge in the corner.
### The message text itself and the suggested fixes belong to the notices-and-errors area. Feature names stay untranslated.

review-ok = Computing normally now
review-close = Close
review-close-tip = Esc also closes
# $name: the earlier feature that blocks this one
review-blocked-by = Blocked by: { $name }
review-fixes = Ways to fix it
# Heading of the original message with internal terms (for bug reports)
review-raw = Original message
# Second line of the copied text. $raw: the original message
review-raw-line = Original message: { $raw }
review-no-problem = This feature has no warnings or errors
review-edit = Edit
review-copy = Copy message
review-copy-tip = Copy the feature name and the message (with the original message) to the clipboard
review-pick-cause = Select the cause feature
review-copied = Copied

## Badge in the bottom-right corner

# Badge text. $errors, $warnings: counts (at least one is 1 or more)
review-badge =
    { $errors ->
        [0] { $warnings ->
            [one] 1 warning
           *[other] { $warnings } warnings
        }
       *[other] { $warnings ->
            [0] { $errors ->
                [one] 1 error
               *[other] { $errors } errors
            }
           *[other] { $errors ->
                [one] 1 error
               *[other] { $errors } errors
            }, { $warnings ->
                [one] 1 warning
               *[other] { $warnings } warnings
            }
        }
    }
# Badge tooltip. $badge: the badge text (review-badge)
review-badge-tip = { $badge }. Press to open the review window; press again to go to the next feature
