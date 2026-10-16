package com.example.app;

import com.example.core.MessageQueue;
import com.example.core.ViewManager;
import com.example.util.Strings;

public class ViewUser {
    private ViewManager viewManager;

    void go() {
        viewManager.setCurrentView();
        MessageQueue q = new MessageQueue();
        q.update();
        foo().bar();
        factory().buildWidgetOnce();
        Strings.trim("x");
    }
}
