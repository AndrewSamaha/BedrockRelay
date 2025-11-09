# Compare Feature

## Overview

The compare feature allows users to mark a packet as a baseline and then compare other packets against it. When in compare mode, the UI splits into two panels: the left panel shows the current packet's full JSON, and the right panel shows only the differences from the baseline packet. This makes it easy to identify changes between packets while still being able to see the full context of the current packet.

## User Experience

### Entering Compare Mode

- **Keybinding**: Press `c` while viewing a packet
- **Action**: Marks the currently displayed packet as the baseline for comparison
- **Visual Feedback**: 
  - The baseline packet is highlighted in yellow on the timeline
  - The header shows "[Compare Mode | Baseline: Packet N]"
  - The timeline title shows the baseline packet number

### Compare Mode Behavior

When in compare mode:

1. **Baseline Packet**: The packet marked with `c` becomes the baseline for all comparisons
2. **Navigation**: Users can navigate to other packets using standard navigation keys (`←`, `→`, `h`, `l`, `PageUp`, `PageDown`, `Home`, `End`)
3. **Two-Panel Layout**:
   - **Left Panel (Packet Details)**: Shows the full JSON of the current packet being viewed (normal view)
   - **Right Panel (Differences)**: Shows only the JSON fields and values that differ from the baseline
4. **Difference Highlighting**: 
   - JSON fields and values that differ from the baseline are highlighted with colors:
     - **Green** for additions (fields in current packet, not in baseline)
     - **Red** for removals (fields in baseline, not in current packet)
     - **Red** for modifications (fields with different values - shows old value in red, new value in green)
   - Fields that are identical to the baseline are hidden by default
   - Only differing fields and their values are displayed
5. **Metadata Deltas**: The differences panel shows:
   - **Time delta**: Difference in time between baseline and current packet (in seconds, with +/- prefix)
   - **Packet number delta**: Difference in packet numbers between baseline and current packet (with +/- prefix)
6. **Timeline Indicator**: The baseline packet is highlighted in yellow on the timeline so users can easily reference which packet is being used for comparison

### Exiting Compare Mode

- **Keybinding**: Press `Esc` (Escape)
- **Action**: Exits compare mode and returns to normal packet viewing
- **Result**: 
  - The two-panel layout collapses back to a single full-width panel
  - All fields are displayed normally
  - Difference highlighting is removed

## Technical Implementation

### State Management

- `compare_mode: bool` - Tracks whether compare mode is active
- `baseline_packet_index: Option<usize>` - Stores the index of the baseline packet
- `baseline_packet_json: Option<serde_json::Value>` - Stores the JSON of the baseline packet for comparison
- `diff_panel_scroll: u16` - Separate scroll state for the differences panel

### Comparison Logic

- Implemented recursive JSON diffing using `BTreeMap` for deterministic, sorted field ordering
- Compares current packet's JSON structure against baseline packet's JSON structure
- Identifies:
  - Fields present in current packet but not in baseline (additions) - shown in green
  - Fields present in baseline but not in current packet (deletions) - shown in red
  - Fields with different values (modifications) - shows old value in red, new value in green
  - Fields with identical values (unchanged - hidden by default)

### Display Logic

- **Two-Panel Layout**: When in compare mode (and not in hex view), the packet details area splits horizontally:
  - Left panel: 50% width - shows full packet JSON
  - Right panel: 50% width - shows differences only
- **Default View**: Show only differing fields and their values in the right panel
- **Visual Highlighting**: Uses color coding:
  - Green for additions
  - Red for deletions and old values in modifications
  - Green for new values in modifications
- **JSON Structure**: Preserves JSON hierarchy and indentation for displayed differences
- **Field Paths**: Shows full JSON paths for nested differences (e.g., "field.subfield.key")
- **Metadata Deltas**: Displays time and packet number deltas at the top of the differences panel in cyan

### Timeline Integration

- Baseline packet is highlighted in yellow on the timeline
- When the current packet is also the baseline, it shows yellow with bold and reversed styling
- Timeline title shows "Baseline: Packet N" when in compare mode
- Indicator remains visible while in compare mode

### Edge Cases Handled

- **Empty Packets**: Handles comparison when baseline or current packet has empty JSON
- **Different Structures**: Handles cases where packets have completely different JSON structures
- **Nested Objects**: Properly compares nested JSON objects recursively
- **Array Differences**: Identifies differences in arrays (added/removed/modified elements)
- **Type Changes**: Handles cases where field types differ (e.g., string vs number)
- **Baseline Packet Viewing**: When viewing the baseline packet itself, shows a message indicating it's the baseline
- **No Differences**: Shows "No differences from baseline packet." when packets are identical
- **Hex View**: Compare mode is disabled in hex view (only works in JSON view)

## UI/UX Implementation

### Visual Design

- Color coding for different types of differences:
  - **Green** for additions and new values in modifications
  - **Red** for deletions and old values in modifications
  - **Cyan** for metadata deltas (time and packet number)
  - **Yellow** for baseline packet indicators
- Colors are distinguishable and work in standard terminal color schemes

### Keyboard Shortcuts

- `c` - Enter compare mode / Set baseline (when not in compare mode)
- `Esc` - Exit compare mode (or return to session list if not in compare mode)
- All standard navigation keys continue to work in compare mode
- Scroll keys (`k`/`j`, `↑`/`↓`) scroll the packet details panel (left)
- Differences panel (right) auto-updates when navigating between packets

### Mode Indication

- Header shows "[Compare Mode | Baseline: Packet N]" when compare mode is active
- Timeline shows baseline packet number in the title
- Baseline packet is visually distinct on the timeline (yellow highlight)

## Implementation Details

### JSON Diffing

- Custom recursive JSON diffing algorithm implemented
- Uses `BTreeMap` instead of `HashMap` for deterministic, sorted field ordering (prevents fields from changing order on each render)
- Handles JSON structure differences gracefully
- Recursively compares nested objects and arrays

### Performance

- Comparison is fast enough for real-time navigation
- Diff is recalculated on each packet navigation (no caching, but performance is acceptable)
- Optimized for typical packet JSON structures

### Integration Points

- Integrated with existing packet viewing code
- Works only in JSON view (disabled in hex view)
- Respects existing filter modes (direction filtering)
- Coordinates with timeline rendering
- Compare mode resets when:
  - Loading a new session
  - Applying a filter
  - Exiting compare mode
  - Returning to session list

## Completed Features

✅ Mark packet as baseline with `c` key  
✅ Two-panel layout (full packet on left, differences on right)  
✅ Color-coded difference highlighting (green/red)  
✅ Hide unchanged fields by default  
✅ Timeline indicator for baseline packet  
✅ Exit compare mode with `Esc`  
✅ Time delta display  
✅ Packet number delta display  
✅ Deterministic field ordering (using BTreeMap)  
✅ Handle edge cases (empty packets, different structures, nested objects, arrays)  
✅ Baseline packet viewing message  
✅ No differences message  

## Future Enhancements (Not Implemented)

- Toggle to show/hide unchanged fields
- Ability to change baseline packet without exiting compare mode
- Scroll support for differences panel (currently auto-updates on navigation)
- Export comparison results
- Compare multiple packets at once
- Highlight specific field paths of interest
- Caching of comparison results for better performance with large packet sets
