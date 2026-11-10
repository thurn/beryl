// Reads image paths from stdin, one per line; writes one JSON line per image with the text
// Apple's Vision framework recognizes in it: {"path", "lines": [{"text", "x", "y", "w", "h"}]},
// boxes as fractions of the image from its top left.
import Foundation
import Vision
import ImageIO

func recognize(_ path: String) -> [[String: Any]] {
    let url = URL(fileURLWithPath: path)
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
        let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else { return [] }
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = false
    request.recognitionLanguages = ["en-US"]
    try? VNImageRequestHandler(cgImage: image).perform([request])
    return (request.results ?? []).compactMap { observation in
        guard let best = observation.topCandidates(1).first else { return nil }
        let box = observation.boundingBox
        return [
            "text": best.string,
            "x": (box.minX * 10000).rounded() / 10000,
            "y": ((1 - box.maxY) * 10000).rounded() / 10000,
            "w": (box.width * 10000).rounded() / 10000,
            "h": (box.height * 10000).rounded() / 10000,
        ]
    }
}

var paths: [String] = []
while let line = readLine() { if !line.isEmpty { paths.append(line) } }
let lock = NSLock()
DispatchQueue.concurrentPerform(iterations: paths.count) { index in
    let record: [String: Any] = ["path": paths[index], "lines": recognize(paths[index])]
    let data = try! JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])
    lock.lock()
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write("\n".data(using: .utf8)!)
    lock.unlock()
}
