import Foundation
import Metal
import MetalKit
import UIKit

// The following C-style functions should be automatically imported 
// through the bridging header ABetterWorldIOS-Bridging-Header.h
// If you're seeing "Cannot find 'abetterworld_ios_new' in scope" errors,
// make sure in Xcode:
// 1. "ABetterWorldIOS-Bridging-Header.h" is set as your Swift Compiler - 
//    General > Objective-C Bridging Header
// 2. The Rust static library is properly linked in Build Phases > Link Binary With Libraries
// 3. The header search paths include the path to ABetterWorldIOS.h

public class ABetterWorldRenderer {
    private var renderer: UnsafeMutablePointer<ABetterWorldiOS>?
    private var metalDevice: MTLDevice
    private var metalLayer: CAMetalLayer
    
    public init?(metalView: MTKView) {
        print("Initializing ABetterWorldRenderer")
        
        guard let device = MTLCreateSystemDefaultDevice() else {
            print("ERROR: Metal is not supported on this device")
            return nil
        }
        print("Got Metal device: \(device)")
        
        self.metalDevice = device
        metalView.device = device
        
        guard let layer = metalView.layer as? CAMetalLayer else {
            print("ERROR: Metal view doesn't have a CAMetalLayer")
            return nil
        }
        print("Got Metal layer")
        
        metalView.drawableSize = CGSize(
            width: metalView.bounds.width * metalView.contentScaleFactor,
            height: metalView.bounds.height * metalView.contentScaleFactor
        )
        
        // Log dimensions
        let width = Double(metalView.bounds.width)
        let height = Double(metalView.bounds.height)
        print("Initializing with dimensions: \(width) x \(height)")
        
        self.metalLayer = layer
        layer.pixelFormat = .bgra8Unorm
        layer.framebufferOnly = false

        // Add these debugging settings
        layer.backgroundColor = CGColor(red: 1.0, green: 0.0, blue: 0.0, alpha: 0.3) // Red tint
        print("Layer frame: \(layer.frame)")
        print("Layer bounds: \(layer.bounds)")
        
        // Create the Rust renderer
        print("Creating Rust renderer")
        self.renderer = abetterworld_ios_new()
        if self.renderer == nil {
            print("ERROR: Failed to create Rust renderer")
            return nil
        }
        print("Rust renderer created")
    
        
        // Initialize with the metal device and layer
        abetterworld_ios_init(
            self.renderer,
            Unmanaged.passUnretained(device).toOpaque(),
            Unmanaged.passUnretained(layer).toOpaque(),
            1320,
            2868
        )
        
        print("Rust renderer initialized")
        
        // Log the version
        if let versionPtr = abetterworld_ios_version() {
            let version = String(cString: versionPtr)
            print("Initialized version: \(version)")
        } else {
            print("WARNING: Could not get version")
        }
    }
    
    deinit {
        if let renderer = self.renderer {
            abetterworld_ios_free(renderer)
        }
    }
    
    public func resize(width: Double, height: Double) {
        guard let renderer = self.renderer else { return }
        abetterworld_ios_resize(renderer, width, height)
    }
    
    public func render() {
        guard let renderer = self.renderer else { return }
        abetterworld_ios_render(renderer)
    }
    
    func renderFallback() {
        guard let drawable = metalLayer.nextDrawable() else {
            print("Failed to get drawable")
            return
        }
        
        let commandBuffer = metalDevice.makeCommandQueue()?.makeCommandBuffer()
        
        let renderPassDescriptor = MTLRenderPassDescriptor()
        renderPassDescriptor.colorAttachments[0].texture = drawable.texture
        renderPassDescriptor.colorAttachments[0].loadAction = .clear
        renderPassDescriptor.colorAttachments[0].clearColor = MTLClearColor(red: 0.0, green: 0.5, blue: 1.0, alpha: 1.0)
        renderPassDescriptor.colorAttachments[0].storeAction = .store
        
        let encoder = commandBuffer?.makeRenderCommandEncoder(descriptor: renderPassDescriptor)
        encoder?.endEncoding()
        
        commandBuffer?.present(drawable)
        commandBuffer?.commit()
        
        print("Fallback render completed")
    }
}

// MARK: - MTKViewDelegate implementation for easy integration

public class ABetterWorldViewDelegate: NSObject, MTKViewDelegate {
    private var renderer: ABetterWorldRenderer?
    
    public init(metalView: MTKView) {
        super.init()
        self.renderer = ABetterWorldRenderer(metalView: metalView)
        metalView.delegate = self
    }
    
    public func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {
        renderer?.resize(width: Double(size.width), height: Double(size.height))
    }
    
    public func draw(in view: MTKView) {
        print("Drawing frame...")
        // Toggle between Rust and fallback
        if !UserDefaults.standard.bool(forKey: "useFallback") {
            renderer?.render()
            print("Rust Frame rendered")
        } else {
            (renderer as? ABetterWorldRenderer)?.renderFallback()
            print("Fallback frame rendered")
        }
    }
}

// MARK: - Simple UIView subclass for even easier integration

public class ABetterWorldView: UIView {
    private var metalView: MTKView!
    private var viewDelegate: ABetterWorldViewDelegate?
    
    public override class var layerClass: AnyClass {
        return CAMetalLayer.self
    }
    
    public override init(frame: CGRect) {
        super.init(frame: frame)
        setupView()
    }
    
    required init?(coder: NSCoder) {
        super.init(coder: coder)
        setupView()
    }
    
    private func setupView() {
        print("Setting up ABetterWorldView")
        
        metalView = MTKView(frame: bounds)
        metalView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        
        // Add border to see if metal view is placed correctly
        metalView.layer.borderWidth = 2
        metalView.layer.borderColor = UIColor.green.cgColor
        
        addSubview(metalView)
        print("Metal view added to hierarchy")
        
        viewDelegate = ABetterWorldViewDelegate(metalView: metalView)
        print("Delegate created")
    }
}
