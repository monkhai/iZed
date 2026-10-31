package com.monkhai.ized.platform;

import java.net.NetworkInterface;
import java.net.InterfaceAddress;
import java.util.Enumeration;
import org.json.JSONArray;
import org.json.JSONObject;

/** SDK network interface enumeration for tsnet; no privileged netlink access. */
public final class TerminalNetwork {
    private TerminalNetwork() {}
    public static String interfaces() throws Exception {
        JSONArray result = new JSONArray();
        Enumeration<NetworkInterface> interfaces = NetworkInterface.getNetworkInterfaces();
        while (interfaces != null && interfaces.hasMoreElements()) {
            NetworkInterface item = interfaces.nextElement();
            JSONObject value = new JSONObject();
            value.put("Name", item.getName());
            value.put("Index", item.getIndex());
            value.put("MTU", item.getMTU());
            int flags = (item.isUp() ? 1 : 0) | (item.isLoopback() ? 4 : 0)
                    | (item.isPointToPoint() ? 8 : 0) | (item.supportsMulticast() ? 16 : 0);
            value.put("Flags", flags);
            JSONArray addresses = new JSONArray();
            for (InterfaceAddress address : item.getInterfaceAddresses()) {
                String ip = address.getAddress().getHostAddress().split("%", 2)[0];
                addresses.put(ip + "/" + address.getNetworkPrefixLength());
            }
            value.put("Addresses", addresses);
            result.put(value);
        }
        return result.toString();
    }
}
