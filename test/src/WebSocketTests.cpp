/**
 * @file WebSocketTests.cpp
 *
 * This module contains the unit tests of the
 * WebSockets::WebSocket class.
 *
 * © 2018 by Richard Walters
 */

#include <gtest/gtest.h>
#include <Http/Connection.hpp>
#include <memory>
#include <stddef.h>
#include <string>
#include <vector>
#include <WebSockets/WebSocket.hpp>

namespace {

    /**
     * This is a fake client connection which is used to test WebSockets.
     */
    struct MockConnection
        : public Http::Connection
    {
        // Properties

        /**
         * This is the delegate to call in order to simulate data coming
         * into the WebSocket from the remote peer.
         */
        DataReceivedDelegate dataReceivedDelegate;

        /**
         * This is the delegate to call in order to simulate closing
         * the connection from the remote peer side.
         */
        BrokenDelegate brokenDelegate;

        /**
         * This holds onto a copy of all data sent by the WebSocket
         * to the remote peer.
         */
        std::string webSocketOutput;

        /**
         * This flag is set if the WebSocket breaks the connection
         * to the remote peer.
         */
        bool brokenByWebSocket = false;

        // Http::Connection

        virtual std::string GetPeerId() override {
            return "mock-client";
        }

        virtual void SetDataReceivedDelegate(DataReceivedDelegate newDataReceivedDelegate) override {
            dataReceivedDelegate = newDataReceivedDelegate;
        }

        virtual void SetBrokenDelegate(BrokenDelegate newBrokenDelegate) override {
            brokenDelegate = newBrokenDelegate;
        }

        virtual void SendData(const std::vector< uint8_t >& data) override {
            (void)webSocketOutput.insert(
                webSocketOutput.end(),
                data.begin(),
                data.end()
            );
        }

        virtual void Break(bool clean) override {
            brokenByWebSocket = true;
        }
    };

}

TEST(WebSocketTests, InitiateOpen) {
}

TEST(WebSocketTests, CompleteOpen) {
}

TEST(WebSocketTests, SendPingNormal) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Ping("Hello");
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("\x89\x05Hello", connection->webSocketOutput);
}

TEST(WebSocketTests, SendPingTooMuchData) {
}

TEST(WebSocketTests, ReceivePing) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Client);
    std::vector< std::string > pings;
    ws.SetPingDelegate(
        [&pings](
            const std::string& data
        ){
            pings.push_back(data);
        }
    );
    const std::string frame = "\x89\x06World!";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        (std::vector< std::string >{
            "World!",
        }),
        pings
    );
    ASSERT_EQ(12, connection->webSocketOutput.length());
    ASSERT_EQ("\x8A\x86", connection->webSocketOutput.substr(0, 2));
    for (size_t i = 0; i < 6; ++i) {
        ASSERT_EQ(
            frame[2 + i] ^ connection->webSocketOutput[2 + (i % 4)],
            connection->webSocketOutput[6 + i]
        );
    }
}

TEST(WebSocketTests, SendPongNormal) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Pong("Hello");
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("\x8A\x05Hello", connection->webSocketOutput);
}

TEST(WebSocketTests, SendPongTooMuchData) {
}

TEST(WebSocketTests, ReceivePong) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Client);
    std::vector< std::string > pongs;
    ws.SetPongDelegate(
        [&pongs](
            const std::string& data
        ){
            pongs.push_back(data);
        }
    );
    const std::string frame = "\x8A\x06World!";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        (std::vector< std::string >{
            "World!",
        }),
        pongs
    );
}

TEST(WebSocketTests, SendText) {
}

TEST(WebSocketTests, ReceiveText) {
}

TEST(WebSocketTests, SendBinary) {
}

TEST(WebSocketTests, ReceiveBinary) {
}

TEST(WebSocketTests, SendMasked) {
}

TEST(WebSocketTests, ReceiveMasked) {
}

TEST(WebSocketTests, SendFragmented) {
}

TEST(WebSocketTests, ReceiveFragmented) {
}

TEST(WebSocketTests, InitiateClose) {
}

TEST(WebSocketTests, CompleteClose) {
}
