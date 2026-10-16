package com.example.core;

public class Bus {
    private Listener listener;

    void fire() {
        listener.onEvent();
        make().onlyInApp();
    }
}
