//
//  ViewController.swift
//  ABetterWorld
//
//  Created by Andrew Tosh on 8/6/25.
//

import UIKit

class ViewController: UIViewController {
    private var sphereView: ABetterWorldView!
    
    override func viewDidLoad() {
        super.viewDidLoad()
        
        // Debug bundle resources
        let bundlePath = Bundle.main.bundlePath
        print("Bundle path: \(bundlePath)")
        
        if let resources = Bundle.main.resourcePath {
            print("Resources path: \(resources)")
            
            // List all files in the bundle
            do {
                let fileManager = FileManager.default
                let files = try fileManager.contentsOfDirectory(atPath: resources)
                print("Bundle contents:")
                for file in files {
                    print("  - \(file)")
                }
            } catch {
                print("Error listing bundle contents: \(error)")
            }
        }
        
        // Set up view as before
        view.backgroundColor = .red
        sphereView = ABetterWorldView(frame: view.bounds)
        sphereView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        view.addSubview(sphereView)
    }
    
    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        // Ensure our view size is updated when the device rotates
        sphereView.frame = view.bounds
    }

    override var prefersStatusBarHidden: Bool {
        return true
    }
    
    override var shouldAutorotate: Bool {
        return true
    }
    
    override var supportedInterfaceOrientations: UIInterfaceOrientationMask {
        return .all
    }


}

