import AppIntents

/// The keys "Send Keyboard Event to Terminal" offers. The raw values are the names the
/// application's key parser accepts.
enum IntentKey: String, AppEnum {
    case a
    case b
    case c
    case d
    case e
    case f
    case g
    case h
    case i
    case j
    case k
    case l
    case m
    case n
    case o
    case p
    case q
    case r
    case s
    case t
    case u
    case v
    case w
    case x
    case y
    case z
    case digit0
    case digit1
    case digit2
    case digit3
    case digit4
    case digit5
    case digit6
    case digit7
    case digit8
    case digit9
    case enter
    case tab
    case space
    case escape
    case backspace
    case delete
    case insert
    case home
    case end
    case pageUp = "page_up"
    case pageDown = "page_down"
    case arrowUp = "arrow_up"
    case arrowDown = "arrow_down"
    case arrowLeft = "arrow_left"
    case arrowRight = "arrow_right"
    case minus
    case equal
    case comma
    case period
    case slash
    case backslash
    case semicolon
    case quote
    case backquote
    case bracketLeft = "bracket_left"
    case bracketRight = "bracket_right"
    case f1
    case f2
    case f3
    case f4
    case f5
    case f6
    case f7
    case f8
    case f9
    case f10
    case f11
    case f12

    static let typeDisplayRepresentation = TypeDisplayRepresentation(name: "Key")

    static let caseDisplayRepresentations: [IntentKey: DisplayRepresentation] = [
        .a: "A",
        .b: "B",
        .c: "C",
        .d: "D",
        .e: "E",
        .f: "F",
        .g: "G",
        .h: "H",
        .i: "I",
        .j: "J",
        .k: "K",
        .l: "L",
        .m: "M",
        .n: "N",
        .o: "O",
        .p: "P",
        .q: "Q",
        .r: "R",
        .s: "S",
        .t: "T",
        .u: "U",
        .v: "V",
        .w: "W",
        .x: "X",
        .y: "Y",
        .z: "Z",
        .digit0: "0",
        .digit1: "1",
        .digit2: "2",
        .digit3: "3",
        .digit4: "4",
        .digit5: "5",
        .digit6: "6",
        .digit7: "7",
        .digit8: "8",
        .digit9: "9",
        .enter: "Enter",
        .tab: "Tab",
        .space: "Space",
        .escape: "Escape",
        .backspace: "Backspace",
        .delete: "Delete",
        .insert: "Insert",
        .home: "Home",
        .end: "End",
        .pageUp: "Page Up",
        .pageDown: "Page Down",
        .arrowUp: "Arrow Up",
        .arrowDown: "Arrow Down",
        .arrowLeft: "Arrow Left",
        .arrowRight: "Arrow Right",
        .minus: "Minus",
        .equal: "Equal",
        .comma: "Comma",
        .period: "Period",
        .slash: "Slash",
        .backslash: "Backslash",
        .semicolon: "Semicolon",
        .quote: "Quote",
        .backquote: "Backquote",
        .bracketLeft: "Left Bracket",
        .bracketRight: "Right Bracket",
        .f1: "F1",
        .f2: "F2",
        .f3: "F3",
        .f4: "F4",
        .f5: "F5",
        .f6: "F6",
        .f7: "F7",
        .f8: "F8",
        .f9: "F9",
        .f10: "F10",
        .f11: "F11",
        .f12: "F12",
    ]
}
