# Enhanced Packet Filtering Feature

## Overview

The enhanced packet filtering feature allows users to filter packets by direction (clientbound/serverbound/all) and optionally by packet type/name. Filters support exact matches and wildcard patterns, and multiple filters can be combined using comma-delimited lists with OR logic. This makes it easy to find specific packets or groups of packets based on their direction and type.

## User Experience

### Entering Filter Mode

- **Keybinding**: Press `f` while viewing packets
- **Action**: Enters filter input mode
- **Visual Feedback**: 
  - Filter input field becomes highlighted (yellow, bold)
  - Cursor appears in the filter input field
  - Help text shows filter format and examples

### Filter Format

The filter format is: `[direction][.packet_name][,filter2,...]`

- **Direction**: 
  - `c` = clientbound packets only
  - `s` = serverbound packets only
  - `a` = all packets (both directions)
- **Packet Name** (optional): 
  - Exact match: `s.player_auth_input` matches only packets with exact name "player_auth_input"
  - Wildcard match: `s.*action*` matches any packet containing "action" in the name (case-insensitive)
  - Wildcard character `*` matches any sequence of characters
- **Multiple Filters**: Comma-delimited list creates OR logic
  - Example: `s.player_auth_input,c.start_game` matches serverbound "player_auth_input" OR clientbound "start_game"

### Filter Examples

#### Basic Direction Filters
- `c` - Show only clientbound packets
- `s` - Show only serverbound packets
- `a` - Show all packets (no filter)

#### Direction + Packet Name (Exact Match)
- `s.player_auth_input` - Serverbound packets with exact name "player_auth_input"
- `c.start_game` - Clientbound packets with exact name "start_game"
- `a.login` - All packets (any direction) with exact name "login"

#### Direction + Packet Name (Wildcard Match)
- `s.*action*` - Serverbound packets containing "action" anywhere in the name (case-insensitive)
- `c.*sleep*` - Clientbound packets containing "sleep" anywhere in the name
- `s.*auth*` - Serverbound packets containing "auth" anywhere in the name
- `c.*game*` - Clientbound packets containing "game" anywhere in the name

#### Multiple Filters (OR Logic)
- `s.player_auth_input,c.start_game` - Serverbound "player_auth_input" OR clientbound "start_game"
- `s.*action*,c.*sleep*` - Serverbound packets with "action" OR clientbound packets with "sleep"
- `s.player_auth_input,s.*action*` - Serverbound "player_auth_input" OR any serverbound packet with "action"

### Filter Mode Behavior

When in filter input mode:

1. **Input Field**: Multi-character input is supported (no longer limited to single character)
2. **Real-time Editing**: 
   - Type characters to build the filter string
   - Use `Backspace` to delete characters
   - Filter string can contain periods, commas, asterisks, and alphanumeric characters
3. **Help Text**: Shows format and examples at the bottom of the filter panel
4. **Visual Feedback**: Input field is highlighted when active

### Applying Filters

- **Keybinding**: Press `Enter` while in filter input mode
- **Action**: 
  - Parses the filter string
  - Applies the filter to the current session
  - Reloads packets matching the filter criteria
  - Preserves packet position when possible (finds closest packet by packet_number)
- **Result**: 
  - Only packets matching the filter are displayed
  - Filter string is shown in the header: `[Filter: s.player_auth_input]`
  - Filter input remains visible showing the applied filter
  - Compare mode is reset (if active)

### Canceling Filters

- **Keybinding**: Press `Esc` while in filter input mode
- **Action**: 
  - Cancels filter input
  - Reverts filter input to currently applied filter (if any)
  - Returns to packet view mode
  - Does not reload packets

### Filter Display

- **Header**: Shows current filter in format `[Filter: filter_string]`
- **Filter Panel**: Always visible, shows current filter input or applied filter
- **Empty Filter**: When no filter is applied, header shows no filter indicator

## Technical Implementation

### Filter Data Structures

#### PacketFilter
```rust
struct PacketFilter {
    direction: Option<FilterPacketDirection>, // None = all directions
    packet_name: Option<String>,              // None = all packet types
    packet_name_is_wildcard: bool,            // true if packet_name contains *
}
```

#### PacketFilterSet
```rust
struct PacketFilterSet {
    filters: Vec<PacketFilter>, // OR logic: packet matches if it matches any filter
}
```

#### DbPacketFilter (Database Layer)
```rust
pub struct DbPacketFilter {
    pub direction: Option<String>,            // "clientbound", "serverbound", or None
    pub packet_name: Option<String>,          // Packet name pattern
    pub packet_name_is_wildcard: bool,        // Use ILIKE vs exact match
}
```

### Filter Parsing

The `parse_filter()` function:

1. **Splits by Comma**: Divides input into individual filter strings
2. **Parses Each Filter**:
   - Extracts direction character (`c`, `s`, `a`, or empty)
   - Extracts packet name (if present, after period delimiter)
   - Detects wildcards by checking for `*` in packet name
3. **Creates FilterSet**: Combines all parsed filters into a `PacketFilterSet`
4. **Validation**: Skips invalid filters (invalid direction characters)

### Database Query Implementation

The database query builder (`get_packets()` in `db.rs`):

1. **Builds Dynamic SQL**: Constructs WHERE clause based on filter set
2. **Direction Filtering**: 
   - Adds `direction = 'clientbound'` or `direction = 'serverbound'` conditions
   - No condition if direction is None (matches all)
3. **Packet Name Filtering**:
   - **Exact Match**: Uses `packet->>'name' = $N` when no wildcards
   - **Wildcard Match**: Uses `packet->>'name' ILIKE $N` when wildcards present
   - Converts `*` to `%` for SQL pattern matching
   - Case-insensitive matching with `ILIKE`
4. **OR Logic**: Combines multiple filters with `OR` operator
5. **Parameter Binding**: Uses parameterized queries for security

### SQL Query Examples

#### Single Exact Match Filter
```sql
SELECT ... FROM packets 
WHERE session_id = $1 
  AND (direction = 'serverbound' AND packet->>'name' = $2)
ORDER BY packet_number ASC
```

#### Single Wildcard Filter
```sql
SELECT ... FROM packets 
WHERE session_id = $1 
  AND (direction = 'serverbound' AND packet->>'name' ILIKE $2)
ORDER BY packet_number ASC
-- $2 = '%action%' (converted from '*action*')
```

#### Multiple Filters (OR)
```sql
SELECT ... FROM packets 
WHERE session_id = $1 
  AND (
    (direction = 'serverbound' AND packet->>'name' = $2)
    OR 
    (direction = 'clientbound' AND packet->>'name' ILIKE $3)
  )
ORDER BY packet_number ASC
-- $2 = 'player_auth_input'
-- $3 = '%sleep%'
```

### State Management

- `filter_input: String` - Current filter input text (editable)
- `current_filter: Option<PacketFilterSet>` - Currently applied filter (parsed)
- Filter state persists when navigating between packets
- Filter resets when loading a new session

### Filter Conversion

The `to_db_filter_set()` method converts UI filter structures to database filter structures:

- Maps `FilterPacketDirection` enum to string values
- Preserves packet name and wildcard flag
- Creates `DbPacketFilterSet` for database queries

### Filter Display

The `to_string()` method formats filter sets for display:

- Converts direction to single character (`c`, `s`, `a`)
- Formats as `direction.packet_name` or just `direction`
- Joins multiple filters with commas
- Used in header display and filter input initialization

## UI/UX Implementation

### Visual Design

- **Filter Panel**: Always visible below header, shows current filter
- **Input Highlighting**: Yellow, bold text when in filter input mode
- **Help Text**: Gray text showing format and examples
- **Header Display**: Shows applied filter in format `[Filter: filter_string]`

### Keyboard Shortcuts

- `f` - Enter filter input mode
- `Enter` - Apply filter (in filter input mode)
- `Esc` - Cancel filter input (in filter input mode)
- `Backspace` - Delete character (in filter input mode)
- All characters - Add to filter string (in filter input mode)

### Mode Indication

- Filter input mode: Input field highlighted, cursor visible
- Applied filter: Shown in header and filter panel
- No filter: Header shows no filter indicator

### Panel Layout

- Filter panel height: 6 lines (increased to accommodate longer help text)
- Input field: 3 lines (with border)
- Help text: 3 lines (wrapped)

## Edge Cases Handled

- **Empty Filter String**: Returns `None` (no filter applied)
- **Invalid Direction**: Skips invalid filter, continues parsing others
- **Empty Packet Name**: Treats as direction-only filter
- **Wildcard in Exact Context**: Detects `*` and uses ILIKE appropriately
- **Multiple Wildcards**: All `*` characters converted to `%`
- **No Matching Packets**: Shows error message, preserves filter state
- **Filter During Compare Mode**: Resets compare mode when filter is applied
- **Filter During Navigation**: Filter persists across packet navigation
- **Session Change**: Filter resets when loading new session

## Integration Points

- Integrated with existing packet viewing code
- Works with all packet viewing modes (JSON, hex)
- Coordinates with compare mode (resets when filter applied)
- Preserves packet position when possible (finds closest packet_number)
- Works with timeline visualization
- Compatible with protocol parser (packet names from database JSON)

## Performance Considerations

- **Database Indexing**: Uses existing indexes on `session_id` and `direction`
- **JSONB Queries**: Uses PostgreSQL JSONB operators (`->>'name'`) for efficient packet name filtering
- **ILIKE Performance**: Wildcard queries use case-insensitive pattern matching
- **Parameter Binding**: All queries use parameterized statements for security and performance
- **Query Optimization**: PostgreSQL query planner optimizes OR conditions

## Completed Features

✅ Direction filtering (clientbound, serverbound, all)  
✅ Packet name filtering (exact match)  
✅ Packet name filtering (wildcard match with `*`)  
✅ Case-insensitive wildcard matching  
✅ Multiple filters with OR logic (comma-delimited)  
✅ Multi-character filter input  
✅ Filter display in header  
✅ Filter persistence during navigation  
✅ Filter reset on session change  
✅ Help text with examples  
✅ Error handling for invalid filters  
✅ Packet position preservation when filtering  
✅ Integration with compare mode  

## Future Enhancements (Not Implemented)

- Filter history (remember recent filters)
- Filter presets/saved filters
- Negation support (e.g., `s.!player_auth_input` to exclude)
- Regex support for more complex patterns
- Filter by packet ID or other packet fields
- Filter by time range or packet number range
- Filter statistics (show count of matching packets)
- Filter validation feedback (show invalid patterns before applying)
- Auto-complete for packet names
- Filter export/import
