package com.example.app;

public class Nest {
    static class Entry {}

    static class Holder {
        Entry e;

        static class Entry {}
    }
}
