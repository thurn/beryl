// The native text field behind a field the library draws, so that editing it shows the
// keyboard. The library asks for it with BERYL_REQUEST_BEGIN_TEXT_ENTRY and puts it away with
// BERYL_REQUEST_END_TEXT_ENTRY; the field itself stays invisible, since the library draws the
// text, caret and selection.

import UIKit

/// An invisible text view that holds the edited field's contents while the keyboard is up. It
/// hands the whole contents to the library after each change, as `beryl_client_text` takes
/// them, and Return to it as the Enter key, so that the library's field decides whether Return
/// starts a new line or submits.
final class TextEntry: NSObject, UITextViewDelegate {
    let view = UITextView(frame: .zero)
    /// Called with the field's whole new contents.
    var onText: ((String) -> Void)?
    /// Called when Return is pressed.
    var onEnter: (() -> Void)?

    override init() {
        super.init()
        view.delegate = self
        // Drawn by the library: the native view shows nothing, but stays in the window, since a
        // hidden view cannot take the keyboard's focus.
        view.backgroundColor = .clear
        view.textColor = .clear
        view.tintColor = .clear
        view.autocorrectionType = .default
        view.autocapitalizationType = .sentences
        view.isScrollEnabled = false
        view.alpha = 0.02
    }

    /// Shows the keyboard for a field at `rect`, in points, holding `text`. A field that is
    /// already being edited keeps the keyboard, and takes the text only if it differs, so that
    /// a layout that moves the field does not reset what the keyboard is composing.
    func begin(text: String, rect: CGRect) {
        view.frame = rect
        if view.text != text {
            view.text = text
        }
        if !view.isFirstResponder {
            view.becomeFirstResponder()
        }
    }

    func end() {
        if view.isFirstResponder {
            view.resignFirstResponder()
        }
    }

    func textViewDidChange(_ textView: UITextView) {
        onText?(textView.text)
    }

    func textView(
        _ textView: UITextView, shouldChangeTextIn range: NSRange, replacementText text: String
    ) -> Bool {
        guard text == "\n" else { return true }
        onEnter?()
        return false
    }
}

extension TableController {
    /// The height in points of the keyboard over the bottom of the view, or 0 while it is down.
    static func keyboardHeight(_ notification: Notification, in view: UIView) -> CGFloat {
        guard
            let frame = notification.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect,
            let window = view.window
        else { return 0 }
        let local = view.convert(frame, from: window.screen.coordinateSpace)
        return max(0, view.bounds.maxY - local.minY)
    }
}
