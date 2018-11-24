/**
 * @file WebSocketTests.cpp
 *
 * This module contains the unit tests of the
 * WebSockets::WebSocket class.
 *
 * © 2018 by Richard Walters
 */

#include <Base64/Base64.hpp>
#include <gtest/gtest.h>
#include <Http/Connection.hpp>
#include <memory>
#include <Hash/Sha1.hpp>
#include <Hash/Templates.hpp>
#include <stddef.h>
#include <string>
#include <SystemAbstractions/DiagnosticsSender.hpp>
#include <SystemAbstractions/StringExtensions.hpp>
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

        virtual std::string GetPeerAddress() override {
            return "mock-client";
        }

        virtual std::string GetPeerId() override {
            return "mock-client:5555";
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

/**
 * This is the test fixture for these tests, providing common
 * setup and teardown for each test.
 */
struct WebSocketTests
    : public ::testing::Test
{
    // Properties

    /**
     * This is the unit under test.
     */
    WebSockets::WebSocket ws;

    /**
     * This flag is used to tell the test fixture if we
     * moved the unit under test.
     */
    bool wsWasMoved = false;

    /**
     * These are the diagnostic messages that have been
     * received from the unit under test.
     */
    std::vector< std::string > diagnosticMessages;

    /**
     * This is the delegate obtained when subscribing
     * to receive diagnostic messages from the unit under test.
     * It's called to terminate the subscription.
     */
    SystemAbstractions::DiagnosticsSender::UnsubscribeDelegate diagnosticsUnsubscribeDelegate;

    // Methods

    /**
     * This method cleanly destroys the old WebSocket and sets up
     * a new one that works in this test fixture.
     */
    void ReplaceWebSocket() {
        TearDown();
        ws = WebSockets::WebSocket();
        wsWasMoved = false;
        SetUp();
    }

    // ::testing::Test

    virtual void SetUp() {
        diagnosticsUnsubscribeDelegate = ws.SubscribeToDiagnostics(
            [this](
                std::string senderName,
                size_t level,
                std::string message
            ){
                diagnosticMessages.push_back(
                    SystemAbstractions::sprintf(
                        "%s[%zu]: %s",
                        senderName.c_str(),
                        level,
                        message.c_str()
                    )
                );
            },
            0
        );
    }

    virtual void TearDown() {
        if (!wsWasMoved) {
            diagnosticsUnsubscribeDelegate();
        }
    }
};

TEST_F(WebSocketTests, InitiateOpenAsClient) {
    Http::Request request;
    ws.StartOpenAsClient(request);
    EXPECT_EQ("13", request.headers.GetHeaderValue("Sec-WebSocket-Version"));
    ASSERT_TRUE(request.headers.HasHeader("Sec-WebSocket-Key"));
    const auto key = request.headers.GetHeaderValue("Sec-WebSocket-Key");
    EXPECT_EQ(
        key,
        Base64::Encode(Base64::Decode(key))
    );
    EXPECT_EQ(
        "websocket",
        SystemAbstractions::ToLower(request.headers.GetHeaderValue("Upgrade"))
    );
    EXPECT_TRUE(request.headers.HasHeaderToken("Connection", "upgrade"));
}

TEST_F(WebSocketTests, FailCompleteOpenAsClientDueToMissingUpgrade) {
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Encode(
            Hash::StringToBytes< Hash::Sha1 >(
                request.headers.GetHeaderValue("Sec-WebSocket-Key")
                + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
            )
        )
    );
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.FinishOpenAsClient(
            connection,
            response
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsClientDueToWrongUpgrade) {
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "foobar");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Encode(
            Hash::StringToBytes< Hash::Sha1 >(
                request.headers.GetHeaderValue("Sec-WebSocket-Key")
                + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
            )
        )
    );
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.FinishOpenAsClient(
            connection,
            response
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsClientDueToMissingConnection) {
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Encode(
            Hash::StringToBytes< Hash::Sha1 >(
                request.headers.GetHeaderValue("Sec-WebSocket-Key")
                + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
            )
        )
    );
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.FinishOpenAsClient(
            connection,
            response
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsClientDueToWrongConnection) {
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "foobar");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Encode(
            Hash::StringToBytes< Hash::Sha1 >(
                request.headers.GetHeaderValue("Sec-WebSocket-Key")
                + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
            )
        )
    );
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.FinishOpenAsClient(
            connection,
            response
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsClientDueToMissingAccept) {
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.FinishOpenAsClient(
            connection,
            response
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsClientDueToWrongAccept) {
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Encode(
            Hash::StringToBytes< Hash::Sha1 >(
                request.headers.GetHeaderValue("Sec-WebSocket-Key")
                + "258EAFA5-E914-47DA-95CA-C5AB0DC85B12"
            )
        )
    );
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.FinishOpenAsClient(
            connection,
            response
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsClientDueToUnsupportedExtension) {
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Encode(
            Hash::StringToBytes< Hash::Sha1 >(
                request.headers.GetHeaderValue("Sec-WebSocket-Key")
                + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
            )
        )
    );
    response.headers.SetHeader("Sec-WebSocket-Extensions", "foobar");
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.FinishOpenAsClient(
            connection,
            response
        )
    );
}

TEST_F(WebSocketTests, SucceedCompleteOpenAsClientBlankExtensions) {
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Encode(
            Hash::StringToBytes< Hash::Sha1 >(
                request.headers.GetHeaderValue("Sec-WebSocket-Key")
                + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
            )
        )
    );
    response.headers.SetHeader("Sec-WebSocket-Extensions", "");
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_TRUE(
        ws.FinishOpenAsClient(
            connection,
            response
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsClientDueToUnsupportedProtocol) {
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Encode(
            Hash::StringToBytes< Hash::Sha1 >(
                request.headers.GetHeaderValue("Sec-WebSocket-Key")
                + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
            )
        )
    );
    response.headers.SetHeader("Sec-WebSocket-Protocol", "foobar");
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.FinishOpenAsClient(
            connection,
            response
        )
    );
}

TEST_F(WebSocketTests, SucceedCompleteOpenAsClientBlankProtocol) {
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Encode(
            Hash::StringToBytes< Hash::Sha1 >(
                request.headers.GetHeaderValue("Sec-WebSocket-Key")
                + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
            )
        )
    );
    response.headers.SetHeader("Sec-WebSocket-Protocol", "");
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_TRUE(
        ws.FinishOpenAsClient(
            connection,
            response
        )
    );
    ReplaceWebSocket();
    ws.StartOpenAsClient(request);
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Encode(
            Hash::StringToBytes< Hash::Sha1 >(
                request.headers.GetHeaderValue("Sec-WebSocket-Key")
                + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
            )
        )
    );
    response.headers.SetHeader("Sec-WebSocket-Protocol", " ");
    ASSERT_TRUE(
        ws.FinishOpenAsClient(
            connection,
            response
        )
    );
}

TEST_F(WebSocketTests, CompleteOpenAsClient) {
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Encode(
            Hash::StringToBytes< Hash::Sha1 >(
                request.headers.GetHeaderValue("Sec-WebSocket-Key")
                + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
            )
        )
    );
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_TRUE(
        ws.FinishOpenAsClient(
            connection,
            response
        )
    );
    const std::string data = "Hello";
    ws.Ping(data);
    ASSERT_EQ(data.length() + 6, connection->webSocketOutput.length());
    ASSERT_EQ("\x89\x85", connection->webSocketOutput.substr(0, 2));
    for (size_t i = 0; i < data.length(); ++i) {
        ASSERT_EQ(
            data[i] ^ connection->webSocketOutput[2 + (i % 4)],
            connection->webSocketOutput[6 + i]
        );
    }
}

TEST_F(WebSocketTests, CompleteOpenAsServer) {
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = "dGhlIHNhbXBsZSBub25jZQ==";
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    Http::Response response;
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_TRUE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
    EXPECT_EQ(101, response.statusCode);
    EXPECT_EQ("Switching Protocols", response.reasonPhrase);
    EXPECT_EQ(
        "websocket",
        SystemAbstractions::ToLower(response.headers.GetHeaderValue("Upgrade"))
    );
    EXPECT_TRUE(response.headers.HasHeaderToken("Connection", "upgrade"));
    EXPECT_EQ(
        "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=",
        response.headers.GetHeaderValue("Sec-WebSocket-Accept")
    );
    ws.Ping("Hello");
    ASSERT_EQ("\x89\x05Hello", connection->webSocketOutput);
}

TEST_F(WebSocketTests, CompleteOpenAsServerConnectionTokenCapitalized) {
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "Upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Encode("abcdefghijklmnop");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    Http::Response response;
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_TRUE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
}

TEST_F(WebSocketTests, CompleteOpenAsServerWithTrailer) {
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Encode("abcdefghijklmnop");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    Http::Response response;
    const auto connection = std::make_shared< MockConnection >();
    std::vector< std::string > pongs;
    ws.SetPongDelegate(
        [&pongs](
            const std::string& data
        ){
            pongs.push_back(data);
        }
    );
    ASSERT_TRUE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            "\x8A"
        )
    );
    EXPECT_TRUE(pongs.empty());
    connection->dataReceivedDelegate({ 0x80, 0x12, 0x34, 0x56, 0x78 });
    ASSERT_EQ(
        (std::vector< std::string >{
            "",
        }),
        pongs
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsServerNotGetMethod) {
    Http::Request request;
    request.method = "POST";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Encode("abcdefghijklmnop");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    Http::Response response;
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsServerMissingUpgrade) {
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Encode("abcdefghijklmnop");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    Http::Response response;
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsServerWrongUpgrade) {
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "foobar");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Encode("abcdefghijklmnop");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    Http::Response response;
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsServerMissingConnection) {
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Encode("abcdefghijklmnop");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    Http::Response response;
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsServerWrongConnection) {
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "foobar");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Encode("abcdefghijklmnop");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    Http::Response response;
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsServerMissingKey) {
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    Http::Response response;
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsServerBadKey) {
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    std::string key = Base64::Encode("abcdefghijklmno");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    Http::Response response;
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
    key = Base64::Encode("abcdefghijklmnopq");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    ASSERT_FALSE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
    key = Base64::Encode("abcdefghijklmnop");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    ASSERT_TRUE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsServerMissingVersion) {
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    const std::string key = Base64::Encode("abcdefghijklmnop");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    Http::Response response;
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
}

TEST_F(WebSocketTests, FailCompleteOpenAsServerBadVersion) {
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "12");
    const std::string key = Base64::Encode("abcdefghijklmnop");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    Http::Response response;
    const auto connection = std::make_shared< MockConnection >();
    ASSERT_FALSE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
    request.headers.SetHeader("Sec-WebSocket-Version", "14");
    ASSERT_FALSE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    ASSERT_TRUE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
}

TEST_F(WebSocketTests, SendPingNormalWithData) {
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Ping("Hello");
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("\x89\x05Hello", connection->webSocketOutput);
}

TEST_F(WebSocketTests, SendPingNormalWithoutData) {
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Ping();
    ASSERT_EQ(std::string("\x89\x00", 2), connection->webSocketOutput);
}

TEST_F(WebSocketTests, SendPingAlmostTooMuchData) {
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Ping(std::string(125, 'x'));
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        std::string("\x89\x7D") + std::string(125, 'x'),
        connection->webSocketOutput
    );
}

TEST_F(WebSocketTests, SendPingTooMuchData) {
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Ping(std::string(126, 'x'));
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("", connection->webSocketOutput);
}

TEST_F(WebSocketTests, ReceivePing) {
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

TEST_F(WebSocketTests, SendPongNormalWithData) {
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Pong("Hello");
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("\x8A\x05Hello", connection->webSocketOutput);
}

TEST_F(WebSocketTests, SendPongNormalWithoutData) {
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Pong();
    ASSERT_EQ(std::string("\x8A\x00", 2), connection->webSocketOutput);
}

TEST_F(WebSocketTests, SendPongAlmostTooMuchData) {
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Pong(std::string(125, 'x'));
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        std::string("\x8A\x7D") + std::string(125, 'x'),
        connection->webSocketOutput
    );
}

TEST_F(WebSocketTests, SendPongTooMuchData) {
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.Pong(std::string(126, 'x'));
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("", connection->webSocketOutput);
}

TEST_F(WebSocketTests, ReceivePong) {
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

TEST_F(WebSocketTests, SendText) {
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.SendText("Hello, World!");
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("\x81\x0DHello, World!", connection->webSocketOutput);
}

TEST_F(WebSocketTests, ReceiveText) {
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

TEST_F(WebSocketTests, SendBinary) {
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Server);
    ws.SendBinary("Hello, World!");
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ("\x82\x0DHello, World!", connection->webSocketOutput);
}

TEST_F(WebSocketTests, ReceiveBinary) {
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

TEST_F(WebSocketTests, SendMasked) {
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

TEST_F(WebSocketTests, ReceiveMasked) {
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

TEST_F(WebSocketTests, SendFragmentedText) {
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

TEST_F(WebSocketTests, SendFragmentedBinary) {
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

TEST_F(WebSocketTests, ReceiveFragmentedText) {
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

TEST_F(WebSocketTests, InitiateCloseNoStatusReturned) {
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

TEST_F(WebSocketTests, InitiateCloseStatusReturned) {
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

TEST_F(WebSocketTests, ReceiveClose) {
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
    std::vector< std::string > pongs;
    ws.SetPongDelegate(
        [&pongs](
            const std::string& data
        ){
            pongs.push_back(data);
        }
    );
    std::string frame = "\x88\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_FALSE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1005, codeReceived);
    EXPECT_EQ("", reasonReceived);
    ASSERT_EQ(
        (std::vector< std::string >{
            "WebSockets::WebSocket[1]: Connection to mock-client:5555 closed by peer",
        }),
        diagnosticMessages
    );
    diagnosticMessages.clear();
    ws.Ping();
    ASSERT_EQ(std::string("\x89\x00", 2), connection->webSocketOutput);
    frame = "\x8A\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    EXPECT_TRUE(pongs.empty());
    connection->webSocketOutput.clear();
    ws.Close();
    ASSERT_EQ(std::string("\x88\x00", 2), connection->webSocketOutput);
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_EQ(
        (std::vector< std::string >{
            "WebSockets::WebSocket[1]: Connection to mock-client:5555 closed ()",
        }),
        diagnosticMessages
    );
    diagnosticMessages.clear();
}

TEST_F(WebSocketTests, ViolationReservedBitsSet) {
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
    std::string frame = "\x99\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_EQ("\x88\x13\x03\xeareserved bits set", connection->webSocketOutput);
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1002, codeReceived);
    EXPECT_EQ("reserved bits set", reasonReceived);
}

TEST_F(WebSocketTests, ViolationUnexpectedContinuation) {
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
    std::string frame = "\x80\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_EQ("\x88\x1F\x03\xeaunexpected continuation frame", connection->webSocketOutput);
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1002, codeReceived);
    EXPECT_EQ("unexpected continuation frame", reasonReceived);
}

TEST_F(WebSocketTests, ViolationNewTextMessageDuringFragmentedMessage) {
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
    std::string frame = "\x01\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_TRUE(connection->webSocketOutput.empty());
    frame = "\x01\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_EQ("\x88\x19\x03\xealast message incomplete", connection->webSocketOutput);
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1002, codeReceived);
    EXPECT_EQ("last message incomplete", reasonReceived);
}

TEST_F(WebSocketTests, ViolationNewBinaryMessageDuringFragmentedMessage) {
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
    std::string frame = "\x01\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_TRUE(connection->webSocketOutput.empty());
    frame = "\x02\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_EQ("\x88\x19\x03\xealast message incomplete", connection->webSocketOutput);
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1002, codeReceived);
    EXPECT_EQ("last message incomplete", reasonReceived);
}

TEST_F(WebSocketTests, ViolationUnknownOpcode) {
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
    std::string frame = "\x83\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_EQ("\x88\x10\x03\xeaunknown opcode", connection->webSocketOutput);
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1002, codeReceived);
    EXPECT_EQ("unknown opcode", reasonReceived);
    ASSERT_EQ(
        (std::vector< std::string >{
            "WebSockets::WebSocket[1]: Connection to mock-client:5555 closed (unknown opcode)",
        }),
        diagnosticMessages
    );
}

TEST_F(WebSocketTests, ViolationClientShouldMaskFrames) {
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
    std::string frame = std::string("\x89\x00", 2);
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_EQ("\x88\x10\x03\xeaunmasked frame", connection->webSocketOutput);
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1002, codeReceived);
    EXPECT_EQ("unmasked frame", reasonReceived);
    ASSERT_EQ(
        (std::vector< std::string >{
            "WebSockets::WebSocket[1]: Connection to mock-client:5555 closed (unmasked frame)",
        }),
        diagnosticMessages
    );
}

TEST_F(WebSocketTests, ViolationServerShouldNotMaskFrames) {
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Client);
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
    std::string frame = "\x89\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    const std::string data = "\x03\xeamasked frame";
    ASSERT_EQ(data.length() + 6, connection->webSocketOutput.length());
    ASSERT_EQ("\x88\x8E", connection->webSocketOutput.substr(0, 2));
    for (size_t i = 0; i < data.length(); ++i) {
        ASSERT_EQ(
            data[i] ^ connection->webSocketOutput[2 + (i % 4)],
            connection->webSocketOutput[6 + i]
        );
    }
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1002, codeReceived);
    EXPECT_EQ("masked frame", reasonReceived);
    ASSERT_EQ(
        (std::vector< std::string >{
            "WebSockets::WebSocket[1]: Connection to mock-client:5555 closed (masked frame)",
        }),
        diagnosticMessages
    );
}

TEST_F(WebSocketTests, ConnectionUnexpectedlyBroken) {
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
    connection->brokenDelegate(false);
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1006, codeReceived);
    EXPECT_EQ("connection broken by peer", reasonReceived);
    ASSERT_EQ(
        (std::vector< std::string >{
            "WebSockets::WebSocket[1]: Connection to mock-client:5555 broken by peer",
        }),
        diagnosticMessages
    );
    diagnosticMessages.clear();
}

TEST_F(WebSocketTests, BadUtf8InText) {
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
    std::string frame = "\x81\x82";
    const std::string unmaskedPayload = "\xc0\xaf"; // overly-long encoding of '/'
    const char mask[4] = {0x12, 0x34, 0x56, 0x78};
    frame += std::string(mask, 4);
    for (size_t i = 0; i < unmaskedPayload.length(); ++i) {
        frame += unmaskedPayload[i] ^ mask[i % 4];
    }
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    EXPECT_EQ("\x88\x28\x03\xefinvalid UTF-8 encoding in text message", connection->webSocketOutput);
    EXPECT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1007, codeReceived);
    EXPECT_EQ("invalid UTF-8 encoding in text message", reasonReceived);
}

TEST_F(WebSocketTests, GoodUtf8InTextSplitIntoFragments) {
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
    std::string frame = "\x01\x02\xF0\xA3";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    EXPECT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        (std::vector< std::string >{
        }),
        texts
    );
    frame = "\x80\x02\x8E\xB4";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    EXPECT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        (std::vector< std::string >{
            "𣎴",
        }),
        texts
    );
}

TEST_F(WebSocketTests, BadUtf8TruncatedInTextSplitIntoFragments) {
    const auto connection = std::make_shared< MockConnection >();
    ws.Open(connection, WebSockets::WebSocket::Role::Client);
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
    std::vector< std::string > texts;
    ws.SetTextDelegate(
        [&texts](
            const std::string& data
        ){
            texts.push_back(data);
        }
    );
    std::string frame = "\x01\x02\xF0\xA3";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    EXPECT_FALSE(connection->brokenByWebSocket);
    ASSERT_EQ(
        (std::vector< std::string >{
        }),
        texts
    );
    frame = "\x80\x01\x8E";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_EQ(
        (std::vector< std::string >{
        }),
        texts
    );
    EXPECT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1007, codeReceived);
    EXPECT_EQ("invalid UTF-8 encoding in text message", reasonReceived);
}

TEST_F(WebSocketTests, ReceiveCloseInvalidUtf8InReason) {
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
    std::string frame = "\x88\x84";
    const std::string unmaskedPayload = "\x03\xe8\xc0\xaf"; // overly-long encoding of '/'
    const char mask[4] = {0x12, 0x34, 0x56, 0x78};
    frame += std::string(mask, 4);
    for (size_t i = 0; i < unmaskedPayload.length(); ++i) {
        frame += unmaskedPayload[i] ^ mask[i % 4];
    }
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    EXPECT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1007, codeReceived);
    EXPECT_EQ("invalid UTF-8 encoding in close reason", reasonReceived);
}

TEST_F(WebSocketTests, ReceiveTextAfterMoving) {
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
    WebSockets::WebSocket ws2(std::move(ws));
    wsWasMoved = true;
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
