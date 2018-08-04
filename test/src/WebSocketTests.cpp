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
#include <Sha1/Sha1.hpp>
#include <stddef.h>
#include <string>
#include <SystemAbstractions/StringExtensions.hpp>
#include <vector>
#include <WebSockets/WebSocket.hpp>

namespace {

    /**
     * This function takes a string and swaps all upper-case characters
     * with their lower-case equivalents, returning the result.
     *
     * @param[in] inString
     *     This is the string to be normalized.
     *
     * @return
     *     The normalized string is returned.  All upper-case characters
     *     are replaced with their lower-case equivalents.
     */
    std::string NormalizeCaseInsensitiveString(const std::string& inString) {
        std::string outString;
        for (char c: inString) {
            outString.push_back(tolower(c));
        }
        return outString;
    }

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

TEST(WebSocketTests, InitiateOpenAsClient) {
    WebSockets::WebSocket ws;
    Http::Request request;
    ws.StartOpenAsClient(request);
    EXPECT_EQ("13", request.headers.GetHeaderValue("Sec-WebSocket-Version"));
    ASSERT_TRUE(request.headers.HasHeader("Sec-WebSocket-Key"));
    const auto key = request.headers.GetHeaderValue("Sec-WebSocket-Key");
    EXPECT_EQ(
        key,
        Base64::Base64Encode(Base64::Base64Decode(key))
    );
    EXPECT_EQ(
        "websocket",
        NormalizeCaseInsensitiveString(request.headers.GetHeaderValue("Upgrade"))
    );
    bool foundUpgradeToken = false;
    for (const auto token: request.headers.GetHeaderTokens("Connection")) {
        if (token == "upgrade") {
            foundUpgradeToken = true;
            break;
        }
    }
    EXPECT_TRUE(foundUpgradeToken);
}

TEST(WebSocketTests, FailCompleteOpenAsClientDueToMissingUpgrade) {
    WebSockets::WebSocket ws;
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Base64Encode(
            Sha1::Sha1(
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

TEST(WebSocketTests, FailCompleteOpenAsClientDueToWrongUpgrade) {
    WebSockets::WebSocket ws;
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "foobar");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Base64Encode(
            Sha1::Sha1(
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

TEST(WebSocketTests, FailCompleteOpenAsClientDueToMissingConnection) {
    WebSockets::WebSocket ws;
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Base64Encode(
            Sha1::Sha1(
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

TEST(WebSocketTests, FailCompleteOpenAsClientDueToWrongConnection) {
    WebSockets::WebSocket ws;
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "foobar");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Base64Encode(
            Sha1::Sha1(
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

TEST(WebSocketTests, FailCompleteOpenAsClientDueToMissingAccept) {
    WebSockets::WebSocket ws;
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

TEST(WebSocketTests, FailCompleteOpenAsClientDueToWrongAccept) {
    WebSockets::WebSocket ws;
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Base64Encode(
            Sha1::Sha1(
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

TEST(WebSocketTests, FailCompleteOpenAsClientDueToUnsupportedExtension) {
    WebSockets::WebSocket ws;
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Base64Encode(
            Sha1::Sha1(
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

TEST(WebSocketTests, SucceedCompleteOpenAsClientBlankExtensions) {
    WebSockets::WebSocket ws;
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Base64Encode(
            Sha1::Sha1(
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

TEST(WebSocketTests, FailCompleteOpenAsClientDueToUnsupportedProtocol) {
    WebSockets::WebSocket ws;
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Base64Encode(
            Sha1::Sha1(
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

TEST(WebSocketTests, SucceedCompleteOpenAsClientBlankProtocol) {
    WebSockets::WebSocket ws;
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Base64Encode(
            Sha1::Sha1(
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
    ws = WebSockets::WebSocket();
    ws.StartOpenAsClient(request);
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Base64Encode(
            Sha1::Sha1(
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

TEST(WebSocketTests, CompleteOpenAsClient) {
    WebSockets::WebSocket ws;
    Http::Request request;
    ws.StartOpenAsClient(request);
    Http::Response response;
    response.statusCode = 101;
    response.headers.SetHeader("Connection", "upgrade");
    response.headers.SetHeader("Upgrade", "websocket");
    response.headers.SetHeader(
        "Sec-WebSocket-Accept",
        Base64::Base64Encode(
            Sha1::Sha1(
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

TEST(WebSocketTests, CompleteOpenAsServer) {
    WebSockets::WebSocket ws;
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Base64Encode("abcdefghijklmnop");
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
        NormalizeCaseInsensitiveString(response.headers.GetHeaderValue("Upgrade"))
    );
    bool foundUpgradeToken = false;
    for (const auto token: response.headers.GetHeaderTokens("Connection")) {
        if (token == "upgrade") {
            foundUpgradeToken = true;
            break;
        }
    }
    EXPECT_TRUE(foundUpgradeToken);
    EXPECT_EQ(
        Base64::Base64Encode(
            Sha1::Sha1(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11")
        ),
        response.headers.GetHeaderValue("Sec-WebSocket-Accept")
    );
    ws.Ping("Hello");
    ASSERT_EQ("\x89\x05Hello", connection->webSocketOutput);
}

TEST(WebSocketTests, CompleteOpenAsServerConnectionTokenCapitalized) {
    WebSockets::WebSocket ws;
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "Upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Base64Encode("abcdefghijklmnop");
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

TEST(WebSocketTests, CompleteOpenAsServerWithTrailer) {
    WebSockets::WebSocket ws;
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Base64Encode("abcdefghijklmnop");
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

TEST(WebSocketTests, FailCompleteOpenAsServerNotGetMethod) {
    WebSockets::WebSocket ws;
    Http::Request request;
    request.method = "POST";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Base64Encode("abcdefghijklmnop");
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

TEST(WebSocketTests, FailCompleteOpenAsServerMissingUpgrade) {
    WebSockets::WebSocket ws;
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Base64Encode("abcdefghijklmnop");
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

TEST(WebSocketTests, FailCompleteOpenAsServerWrongUpgrade) {
    WebSockets::WebSocket ws;
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "foobar");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Base64Encode("abcdefghijklmnop");
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

TEST(WebSocketTests, FailCompleteOpenAsServerMissingConnection) {
    WebSockets::WebSocket ws;
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Base64Encode("abcdefghijklmnop");
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

TEST(WebSocketTests, FailCompleteOpenAsServerWrongConnection) {
    WebSockets::WebSocket ws;
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "foobar");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    const std::string key = Base64::Base64Encode("abcdefghijklmnop");
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

TEST(WebSocketTests, FailCompleteOpenAsServerMissingKey) {
    WebSockets::WebSocket ws;
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

TEST(WebSocketTests, FailCompleteOpenAsServerBadKey) {
    WebSockets::WebSocket ws;
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "13");
    std::string key = Base64::Base64Encode("abcdefghijklmno");
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
    key = Base64::Base64Encode("abcdefghijklmnopq");
    request.headers.SetHeader("Sec-WebSocket-Key", key);
    ASSERT_FALSE(
        ws.OpenAsServer(
            connection,
            request,
            response,
            ""
        )
    );
    key = Base64::Base64Encode("abcdefghijklmnop");
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

TEST(WebSocketTests, FailCompleteOpenAsServerMissingVersion) {
    WebSockets::WebSocket ws;
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    const std::string key = Base64::Base64Encode("abcdefghijklmnop");
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

TEST(WebSocketTests, FailCompleteOpenAsServerBadVersion) {
    WebSockets::WebSocket ws;
    Http::Request request;
    request.method = "GET";
    request.headers.SetHeader("Connection", "upgrade");
    request.headers.SetHeader("Upgrade", "websocket");
    request.headers.SetHeader("Sec-WebSocket-Version", "12");
    const std::string key = Base64::Base64Encode("abcdefghijklmnop");
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
    ws.Ping();
    ASSERT_EQ(std::string("\x89\x00", 2), connection->webSocketOutput);
    frame = "\x8A\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    EXPECT_TRUE(pongs.empty());
    connection->webSocketOutput.clear();
    ws.Close();
    ASSERT_EQ(std::string("\x88\x00", 2), connection->webSocketOutput);
    ASSERT_TRUE(connection->brokenByWebSocket);
}

TEST(WebSocketTests, ViolationReservedBitsSet) {
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
    std::string frame = "\x99\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_EQ("\x88\x13\x03\xeareserved bits set", connection->webSocketOutput);
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1002, codeReceived);
    EXPECT_EQ("reserved bits set", reasonReceived);
}

TEST(WebSocketTests, ViolationUnexpectedContinuation) {
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
    std::string frame = "\x80\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_EQ("\x88\x1F\x03\xeaunexpected continuation frame", connection->webSocketOutput);
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1002, codeReceived);
    EXPECT_EQ("unexpected continuation frame", reasonReceived);
}

TEST(WebSocketTests, ViolationNewTextMessageDuringFragmentedMessage) {
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

TEST(WebSocketTests, ViolationNewBinaryMessageDuringFragmentedMessage) {
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

TEST(WebSocketTests, ViolationUnknownOpcode) {
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
    std::string frame = "\x83\x80XXXX";
    connection->dataReceivedDelegate({frame.begin(), frame.end()});
    ASSERT_EQ("\x88\x10\x03\xeaunknown opcode", connection->webSocketOutput);
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1002, codeReceived);
    EXPECT_EQ("unknown opcode", reasonReceived);
}

TEST(WebSocketTests, ConnectionUnexpectedlyBroken) {
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
    connection->brokenDelegate();
    ASSERT_TRUE(connection->brokenByWebSocket);
    ASSERT_TRUE(closeReceived);
    EXPECT_EQ(1006, codeReceived);
    EXPECT_EQ("connection broken by peer", reasonReceived);
}

TEST(WebSocketTests, BadUtf8InText) {
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

TEST(WebSocketTests, GoodUtf8InTextSplitIntoFragments) {
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

TEST(WebSocketTests, BadUtf8TruncatedInTextSplitIntoFragments) {
    WebSockets::WebSocket ws;
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

TEST(WebSocketTests, ReceiveCloseInvalidUtf8InReason) {
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
