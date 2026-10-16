package com.example.util;

import com.example.core.Service;

public class Leak {
    void use(Service s) {
        s.run();
    }
}
