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
    // TODO
}

TEST(WebSocketTests, CompleteOpen) {
    // TODO
}

TEST(WebSocketTests, SendPingNormalWithData) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Ping("Hello");
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("\x89\x05Hello", connection->webSocketOutput);
}

TEST(WebSocketTests, SendPingNormalWithoutData) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Ping();
    ASSERT_EQ(std::string("\x89\x00", 2), connection->webSocketOutput);
}

TEST(WebSocketTests, SendPingAlmostTooMuchData) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Ping(std::string(125, 'x'));
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        std::string("\x89\x7D") + std::string(125, 'x'),
        connection->webSocketOutput
    );
}

TEST(WebSocketTests, SendPingTooMuchData) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Ping(std::string(126, 'x'));
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("", connection->webSocketOutput);
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

TEST(WebSocketTests, SendPongNormalWithData) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Pong("Hello");
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("\x8A\x05Hello", connection->webSocketOutput);
}

TEST(WebSocketTests, SendPongNormalWithoutData) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Pong();
    ASSERT_EQ(std::string("\x8A\x00", 2), connection->webSocketOutput);
}

TEST(WebSocketTests, SendPongAlmostTooMuchData) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Pong(std::string(125, 'x'));
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        std::string("\x8A\x7D") + std::string(125, 'x'),
        connection->webSocketOutput
    );
}

TEST(WebSocketTests, SendPongTooMuchData) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Pong(std::string(126, 'x'));
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("", connection->webSocketOutput);
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
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.SendText("Hello, World!");
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("\x81\x0DHello, World!", connection->webSocketOutput);
}

TEST(WebSocketTests, ReceiveText) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Client);
    std::vector< std::string > texts;
    ws.SetTextDelegate(
        [&texts](
            const std::string& data
        ){
            texts.push_back(data);
        }
    );
    const std::string frame = "\x81\x06" "foobar";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        (std::vector< std::string >{
            "foobar",
        }),
        texts
    );
}

TEST(WebSocketTests, SendBinary) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.SendBinary("Hello, World!");
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("\x82\x0DHello, World!", connection->webSocketOutput);
}

TEST(WebSocketTests, ReceiveBinary) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Client);
    std::vector< std::string > binaries;
    ws.SetBinaryDelegate(
        [&binaries](
            const std::string& data
        ){
            binaries.push_back(data);
        }
    );
    const std::string frame = "\x82\x06" "foobar";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        (std::vector< std::string >{
            "foobar",
        }),
        binaries
    );
}

TEST(WebSocketTests, SendMasked) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Client);
    const std::string data = "Hello, World!";
    ws.SendText(data);
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(19, connection->webSocketOutput.length());
    ASSERT_EQ("\x81\x8D", connection->webSocketOutput.substr(0, 2));
    for (size_t i = 0; i < 13; ++i) {
        ASSERT_EQ(
            data[i] ^ connection->webSocketOutput[2 + (i % 4)],
            connection->webSocketOutput[6 + i]
        );
    }
}

TEST(WebSocketTests, ReceiveMasked) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    std::vector< std::string > texts;
    ws.SetTextDelegate(
        [&texts](
            const std::string& data
        ){
            texts.push_back(data);
        }
    );
    const char mask[4] = {0x12, 0x34, 0x56, 0x78};
    const std::string data = "foobar";
    std::string frame = "\x81\x86";
    frame += std::string(mask, 4);
    for (size_t i = 0; i < data.length(); ++i) {
        frame += data[i] ^ mask[i % 4];
    }
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        (std::vector< std::string >{
            data,
        }),
        texts
    );
}

TEST(WebSocketTests, SendFragmentedText) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.SendText("Hello,", false);
    ASSERT_EQ("\x01\x06Hello,", connection->webSocketOutput);
    connection->webSocketOutput.clear();
    ws.SendBinary("X", true);
    ASSERT_TRUE(connection->webSocketOutput.empty());
    ws.Ping();
    ASSERT_EQ(std::string("\x89\x00", 2), connection->webSocketOutput);
    connection->webSocketOutput.clear();
    ws.SendBinary("X", false);
    ASSERT_TRUE(connection->webSocketOutput.empty());
    ws.SendText(" ", false);
    ASSERT_EQ(std::string("\x00\x01 ", 3), connection->webSocketOutput);
    connection->webSocketOutput.clear();
    ws.SendText("World!", true);
    ASSERT_EQ("\x80\x06World!", connection->webSocketOutput);
}

TEST(WebSocketTests, SendFragmentedBinary) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.SendBinary("Hello,", false);
    ASSERT_EQ("\x02\x06Hello,", connection->webSocketOutput);
    connection->webSocketOutput.clear();
    ws.SendText("X", true);
    ASSERT_TRUE(connection->webSocketOutput.empty());
    ws.Ping();
    ASSERT_EQ(std::string("\x89\x00", 2), connection->webSocketOutput);
    connection->webSocketOutput.clear();
    ws.SendText("X", false);
    ASSERT_TRUE(connection->webSocketOutput.empty());
    ws.SendBinary(" ", false);
    ASSERT_EQ(std::string("\x00\x01 ", 3), connection->webSocketOutput);
    connection->webSocketOutput.clear();
    ws.SendBinary("World!", true);
    ASSERT_EQ("\x80\x06World!", connection->webSocketOutput);
}

TEST(WebSocketTests, ReceiveFragmentedText) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Client);
    std::vector< std::string > texts;
    ws.SetTextDelegate(
        [&texts](
            const std::string& data
        ){
            texts.push_back(data);
        }
    );
    const std::vector< std::string > frames{
        "\x01\x03" "foo",
        std::string("\x00\x01", 2) + "b",
        "\x80\x02" "ar",
    };
    for (const auto& frame: frames) {
        connection->dataReceivedDelegate({frame.begin(), frame.end()});
    }
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        (std::vector< std::string >{
            "foobar",
        }),
        texts
    );
}

TEST(WebSocketTests, InitiateCloseNoStatusReturned) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    unsigned int codeReceived;
    std::string reasonReceived;
    bool closeReceived = false;
    ws.SetCloseDelegate(
        [&codeReceived, &reasonReceived, &closeReceived](
            unsigned int code,
            const std::string& reason
        ){
            codeReceived = code;
            reasonReceived = reason;
            closeReceived = true;
        }
    );
    ws.Close(1000, "Goodbye!");
    ASSERT_EQ("\x88\x0A\x03\xe8Goodbye!", connection->webSocketOutput);
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_FALSE(closeReceived);
    const std::string frame = "\x88\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1005, codeReceived);
    EXPECT_EQ("", reasonReceived);
}

TEST(WebSocketTests, InitiateCloseStatusReturned) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    unsigned int codeReceived;
    std::string reasonReceived;
    bool closeReceived = false;
    ws.SetCloseDelegate(
        [&codeReceived, &reasonReceived, &closeReceived](
            unsigned int code,
            const std::string& reason
        ){
            codeReceived = code;
            reasonReceived = reason;
            closeReceived = true;
        }
    );
    ws.Close(1000, "Goodbye!");
    ASSERT_EQ("\x88\x0A\x03\xe8Goodbye!", connection->webSocketOutput);
    connection->webSocketOutput.clear();
    ws.SendText("Yo Dawg, I heard you like...");
    ws.SendBinary("Yo Dawg, I heard you like...");
    ws.Ping();
    ws.Pong();
    ws.Close(1000, "Goodbye AGAIN!");
    ASSERT_TRUE(connection->webSocketOutput.empty());
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_FALSE(closeReceived);
    std::string frame = "\x88\x85";
    const std::string unmaskedPayload = "\x03\xe8" "Bye";
    const char mask[4] = {0x12, 0x34, 0x56, 0x78};
    frame += std::string(mask, 4);
    for (size_t i = 0; i < unmaskedPayload.length(); ++i) {
        frame += unmaskedPayload[i] ^ mask[i % 4];
    }
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1000, codeReceived);
    EXPECT_EQ("Bye", reasonReceived);
}

TEST(WebSocketTests, ReceiveClose) {
    WebSockets::WebSocket ws;
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    unsigned int codeReceived;
    std::string reasonReceived;
    bool closeReceived = false;
    ws.SetCloseDelegate(
        [&codeReceived, &reasonReceived, &closeReceived](
            unsigned int code,
            const std::string& reason
        ){
            codeReceived = code;
            reasonReceived = reason;
            closeReceived = true;
        }
    );
    const std::string frame = "\x88\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1005, codeReceived);
    EXPECT_EQ("", reasonReceived);
    ws.Ping();
    ASSERT_EQ(std::string("\x89\x00", 2), connection->webSocketOutput);
    connection->webSocketOutput.clear();
    ws.Close();
    ASSERT_EQ(std::string("\x88\x00", 2), connection->webSocketOutput);
    ASSERT_TRUE(connection->brokenByWebSocket);
}
