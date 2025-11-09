# Compare Feature

## Overview

The compare feature allows users to mark a packet as a baseline and then compare other packets against it. When in compare mode, navigating to different packets will highlight the JSON fields and values that differ from the baseline, making it easy to identify changes between packets.

## User Experience

### Entering Compare Mode

- **Keybinding**: Press `c` while viewing a packet
- **Action**: Marks the currently displayed packet as the baseline for comparison
- **Visual Feedback**: The baseline packet should be visually indicated on the timeline (e.g., highlighted, marked with an indicator, or shown with a distinct color/style)

### Compare Mode Behavior

When in compare mode:

1. **Baseline Packet**: The packet marked with `c` becomes the baseline for all comparisons
2. **Navigation**: Users can navigate to other packets using standard navigation keys (`←`, `→`, `h`, `l`, `PageUp`, `PageDown`, `Home`, `End`)
3. **Difference Highlighting**: 
   - JSON fields and values that differ from the baseline are highlighted (using color, bold, or other visual indicators)
   - Fields that are identical to the baseline are hidden by default
   - Only differing fields and their values are displayed
4. **Timeline Indicator**: The baseline packet remains visually distinct on the timeline so users can easily reference which packet is being used for comparison

### Exiting Compare Mode

- **Keybinding**: Press `Esc` (Escape)
- **Action**: Exits compare mode and returns to normal packet viewing
- **Result**: All fields are displayed normally, and difference highlighting is removed

## Technical Requirements

### State Management

- Track whether compare mode is active (boolean flag)
- Store the baseline packet ID/index for comparison
- Maintain reference to baseline packet data for diffing

### Comparison Logic

- Compare current packet's JSON structure against baseline packet's JSON structure
- Identify:
  - Fields present in current packet but not in baseline (additions)
  - Fields present in baseline but not in current packet (deletions)
  - Fields with different values (modifications)
  - Fields with identical values (unchanged - hidden by default)

### Display Logic

- **Default View**: Show only differing fields and their values
- **Visual Highlighting**: Use distinct styling for:
  - Added fields (fields in current packet, not in baseline)
  - Removed fields (fields in baseline, not in current packet)
  - Modified fields (fields with different values)
- **JSON Structure**: Preserve JSON hierarchy and indentation for displayed differences
- **Field Paths**: Show full JSON paths for nested differences

### Timeline Integration

- Visually mark the baseline packet on the timeline
- Options for indication:
  - Different background color
  - Border or outline
  - Icon or symbol marker
  - Distinct text styling
- Indicator should remain visible while in compare mode

### Edge Cases

- **Empty Packets**: Handle comparison when baseline or current packet has empty JSON
- **Different Structures**: Handle cases where packets have completely different JSON structures
- **Nested Objects**: Properly compare nested JSON objects and arrays
- **Array Differences**: Identify differences in arrays (added/removed/modified elements)
- **Type Changes**: Handle cases where field types differ (e.g., string vs number)

## UI/UX Considerations

### Visual Design

- Use color coding for different types of differences:
  - Green/yellow for additions
  - Red for deletions
  - Orange/yellow for modifications
- Ensure colors are distinguishable and accessible
- Consider terminal color limitations (support both color and monochrome terminals)

### Keyboard Shortcuts

- `c` - Enter compare mode / Set baseline (when not in compare mode)
- `Esc` - Exit compare mode
- All standard navigation keys continue to work in compare mode

### Mode Indication

- Display a visual indicator in the UI when compare mode is active (e.g., status bar, header)
- Show which packet is the baseline (packet number, timestamp, or other identifier)

## Implementation Notes

### JSON Diffing

- Use a JSON diffing algorithm to compare baseline and current packets
- Consider using a library or implementing a recursive diff function
- Handle JSON structure differences gracefully

### Performance

- Comparison should be fast enough for real-time navigation
- Consider caching comparison results if navigating back to previously viewed packets
- Optimize for large JSON structures

### Integration Points

- Integrate with existing packet viewing code
- Work with both JSON and hex views (or disable in hex view)
- Respect existing filter modes (direction filtering)
- Coordinate with timeline rendering

## Future Enhancements (Optional)

- Toggle to show/hide unchanged fields (currently hidden by default)
- Ability to change baseline packet without exiting compare mode
- Side-by-side comparison view
- Export comparison results
- Compare multiple packets at once
- Highlight specific field paths of interest
