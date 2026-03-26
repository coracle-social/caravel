# Components

This file defines the spec components used across `frontend/spec/pages/*.md`.

- **page**: Root container for a route-level screen. Carries route metadata like `path`, auth requirements, and layout shell.
- **nav**: Top navigation area for primary brand and session-aware links.
- **header**: Top content block for a page or section, usually containing a title.
- **section**: Logical grouping of related content within a page.
- **hero**: Prominent introductory content block for a page.
- **heading**: Section heading wrapper that pairs a title with optional supporting copy.
- **title**: Primary label text for a page, section, card, or control.
- **description**: Supporting explanatory text under a title or heading.
- **text**: Generic body text when no more specific semantic component applies.
- **help_text**: Small informational guidance text, usually near forms or auth flows.

- **brand**: Product identity block (name/logo).
- **badge**: Small status/highlight label.
- **status**: Current state indicator for an entity (for example relay status).
- **meta**: Secondary metadata text (IDs, pubkeys, auxiliary attributes).
- **copyright**: Footer legal ownership line.

- **link**: Navigational text/action leading to another route or URL.
- **links**: Group of related link components.
- **back_link**: Explicit “go back” navigation link to a parent/list view.
- **back_button**: Button-style backward navigation within a multi-step screen.
- **button**: Generic clickable action control.
- **submit_button**: Primary form submission control with optional loading label.

- **actions**: Container for action controls associated with a section/card.
- **controls**: Container for filters/search/sort controls.
- **tabs**: Container for tabbed navigation controls.
- **tab**: A selectable tab option within `tabs`.
- **screen**: Named sub-view/state within a page (e.g., a login subflow).
- **modal**: Overlay dialog for focused temporary interaction.

- **form**: Form container for user input and submission behavior.
- **field**: Structured form field abstraction (label + input type + validation intent).
- **input**: Single input control for user-entered values.
- **select**: Dropdown control for choosing one option from many.
- **option**: A selectable value within `select` or plan/method groups.
- **copyable_input**: Read-only input-like field designed for quick copy actions.
- **tooltip_error**: Inline or anchored validation error shown near a specific field.
- **error_message**: Message shown when an action fails.
- **error_state**: Full-state error presentation for failed data loading.
- **loading_state**: Full-state loading presentation while data is being fetched.

- **list**: Repeating collection container.
- **item**: Single element within a repeating collection.
- **grid**: Multi-column layout container for repeated items/cards.
- **card**: Bounded content block used in feature grids and summaries.
- **product**: Specialized card describing an external ecosystem product/integration.
- **relay_detail_card**: Composite card summarizing relay properties and management actions.

- **avatar**: User/profile image representation.
- **bullets**: Container for short point-form highlights.
- **toggle**: On/off control for a boolean feature or policy.
- **toggles**: Group of related boolean toggle controls.
- **plan_selector**: Control used to switch between available relay plans.
- **pricing_table**: Plan comparison and pricing presentation block.
- **member_count**: Display of current member usage for a relay.
- **members**: Plan capacity descriptor (member limits).
- **price**: Plan pricing descriptor.
- **tenant**: Tenant identity/value display in admin-oriented contexts.
- **created_at**: Human-readable creation timestamp display.
- **bolt11**: Lightning invoice string display.
- **qr_code**: Visual QR representation of a connection URI.
- **camera_preview**: Live camera viewport used for scanning QR codes.

## Control directives used in specs

These appear in the Pug-like spec language but are flow directives rather than UI components:

- **if / else**: Conditional branch based on a boolean condition.
- **when**: Conditional rendering keyed off a selected tab/state/value.
