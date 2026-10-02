// Renders icon.svg with AppKit (macOS only; no other SVG renderer is needed).
//   swift render.swift png  <size> <out.png>
//   swift render.swift rgba <size> <out.rgba>   straight-alpha RGBA8, row-major
import AppKit

let args = CommandLine.arguments
guard args.count == 4, let size = Int(args[2]), let svg = NSImage(contentsOfFile: "icon.svg") else {
    fatalError("usage: swift render.swift png|rgba <size> <out> (run inside assets/)")
}
let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8,
    samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
    bytesPerRow: size * 4, bitsPerPixel: 32)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
svg.draw(in: NSRect(x: 0, y: 0, width: size, height: size))
NSGraphicsContext.restoreGraphicsState()

let out = URL(fileURLWithPath: args[3])
switch args[1] {
case "png": try rep.representation(using: .png, properties: [:])!.write(to: out)
case "rgba":
    // The bitmap is premultiplied; iced wants straight alpha.
    var data = Data(count: size * size * 4)
    for y in 0..<size { for x in 0..<size {
        let c = rep.colorAt(x: x, y: y)!.usingColorSpace(.deviceRGB)!
        let o = (y * size + x) * 4
        data[o] = UInt8((c.redComponent * 255).rounded())
        data[o + 1] = UInt8((c.greenComponent * 255).rounded())
        data[o + 2] = UInt8((c.blueComponent * 255).rounded())
        data[o + 3] = UInt8((c.alphaComponent * 255).rounded())
    } }
    try data.write(to: out)
default: fatalError("png or rgba")
}
