/**
 * @file MakeConnectionTests.cpp
 *
 * This module contains the unit tests of the
 * WebSockets::MakeConnection function.
 *
 * © 2018 by Richard Walters
 */

#include <Base64/Base64.hpp>
#include <gtest/gtest.h>
#include <Hash/Sha1.hpp>
#include <Hash/Templates.hpp>
#include <Http/Connection.hpp>
#include <Http/IClient.hpp>
#include <SystemAbstractions/DiagnosticsSender.hpp>
#include <SystemAbstractions/StringExtensions.hpp>
#include <WebSockets/MakeConnection.hpp>

namespace {

    /**
     * This is a fake client connection which is used to test.
     */
    struct MockConnection
        : public Http::Connection
    {
        // Properties

        BrokenDelegate brokenDelegate;
        Http::Request request;

        // Http::Connection

        virtual std::string GetPeerAddress() override {
            return "mock-client";
        }

        virtual std::string GetPeerId() override {
            return "mock-client:5555";
        }

        virtual void SetDataReceivedDelegate(DataReceivedDelegate newDataReceivedDelegate) override {
        }

        virtual void SetBrokenDelegate(BrokenDelegate newBrokenDelegate) override {
            brokenDelegate = newBrokenDelegate;
        }

        virtual void SendData(const std::vector< uint8_t >& data) override {
        }

        virtual void Break(bool clean) override {
        }
    };

    /**
     * This is a fake HTTP client transaction which is used to test.
     */
    struct MockClientTransaction
        : public Http::IClient::Transaction
    {
        // Properties

        bool awaitCompletionResult = true;

        // Http::IClient::Transaction

        virtual bool AwaitCompletion(
            const std::chrono::milliseconds& relativeTime
        ) override {
            return awaitCompletionResult;
        }

        virtual void SetCompletionDelegate(
            std::function< void() > completionDelegate
        ) override {
        }
    };

    /**
     * This is a fake HTTP client which is used to test.
     */
    struct MockClient
        : public Http::IClient
    {
        // Types

        enum class Behaviors {
            SuccessfulConnection,
            UpgradeWithoutEngage,
            ConnectionNotUpgraded,
            UnableToConnect,
            ConnectionBroken,
            ConnectionTimeOut,
            ConnectionAborted,
        };

        // Properties

        Behaviors behavior = Behaviors::SuccessfulConnection;
        std::promise< std::shared_ptr< MockConnection > > connectionPromise;

        // Methods

        std::shared_ptr< MockConnection > AwaitConnection() {
            auto connectionFuture = connectionPromise.get_future();
            if (connectionFuture.wait_for(std::chrono::seconds(1)) != std::future_status::ready) {
                return nullptr;
            }
            return connectionFuture.get();
        }

        // Http::IClient

        virtual SystemAbstractions::DiagnosticsSender::UnsubscribeDelegate SubscribeToDiagnostics(
            SystemAbstractions::DiagnosticsSender::DiagnosticMessageDelegate delegate,
            size_t minLevel = 0
        ) override {
            return []{};
        }

        virtual std::shared_ptr< Transaction > Request(
            Http::Request request,
            bool persistConnection = true,
            UpgradeDelegate upgradeDelegate = nullptr
        ) override {
            const auto transaction = std::make_shared< MockClientTransaction >();
            const auto connection = std::make_shared< MockConnection >();
            connection->request = request;
            switch (behavior) {
                case Behaviors::SuccessfulConnection: {
                    transaction->state = Http::IClient::Transaction::State::Completed;
                    transaction->response.statusCode = 101;
                    transaction->response.headers.SetHeader("Connection", "upgrade");
                    transaction->response.headers.SetHeader("Upgrade", "websocket");
                    transaction->response.headers.SetHeader(
                        "Sec-WebSocket-Accept",
                        Base64::Encode(
                            Hash::StringToBytes< Hash::Sha1 >(
                                request.headers.GetHeaderValue("Sec-WebSocket-Key")
                                + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
                            )
                        )
                    );
                    transaction->response.headers.SetHeader("Sec-WebSocket-Protocol", "");
                    if (upgradeDelegate != nullptr) {
                        upgradeDelegate(transaction->response, connection, "");
                    }
                } break;

                case Behaviors::UpgradeWithoutEngage: {
                    transaction->state = Http::IClient::Transaction::State::Completed;
                    transaction->response.statusCode = 101;
                    transaction->response.headers.SetHeader("Connection", "upgrade");
                    transaction->response.headers.SetHeader("Upgrade", "websocket");
                    if (upgradeDelegate != nullptr) {
                        upgradeDelegate(transaction->response, connection, "");
                    }
                } break;

                case Behaviors::ConnectionNotUpgraded: {
                    transaction->state = Http::IClient::Transaction::State::Completed;
                    transaction->response.statusCode = 404;
                    transaction->response.reasonPhrase = "Not Found";
                } break;

                case Behaviors::UnableToConnect: {
                    transaction->state = Http::IClient::Transaction::State::UnableToConnect;
                } break;

                case Behaviors::ConnectionBroken: {
                    transaction->state = Http::IClient::Transaction::State::Broken;
                } break;

                case Behaviors::ConnectionTimeOut: {
                    transaction->state = Http::IClient::Transaction::State::Timeout;
                } break;

                case Behaviors::ConnectionAborted: {
                    transaction->state = Http::IClient::Transaction::State::InProgress;
                    transaction->awaitCompletionResult = false;
                } break;
            }
            connectionPromise.set_value(connection);
            return transaction;
        }
    };

}

/**
 * This is the test fixture for these tests, providing common
 * setup and teardown for each test.
 */
struct MakeConnectionTests
    : public ::testing::Test
{
    // Properties

    std::shared_ptr< MockClient > mockClient = std::make_shared< MockClient >();
    std::shared_ptr< SystemAbstractions::DiagnosticsSender > diagnosticsSender = std::make_shared< SystemAbstractions::DiagnosticsSender >("MakeConnection");
    std::vector< std::string > diagnosticMessages;
    SystemAbstractions::DiagnosticsSender::UnsubscribeDelegate diagnosticsUnsubscribeDelegate;

    // Methods

    // ::testing::Test

    virtual void SetUp() {
        diagnosticsUnsubscribeDelegate = diagnosticsSender->SubscribeToDiagnostics(
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
        diagnosticsUnsubscribeDelegate();
    }
};

TEST_F(MakeConnectionTests, ConnectionIsAbortable) {
    // Arrange

    // Act
    const auto results = WebSockets::MakeConnection(
        mockClient,
        "foobar",
        1234,
        diagnosticsSender
    );

    // Assert
    EXPECT_FALSE(results.abortConnection == nullptr);
}

TEST_F(MakeConnectionTests, SuccessfulConnection) {
    // Arrange
    mockClient->behavior = MockClient::Behaviors::SuccessfulConnection;

    // Act
    auto results = WebSockets::MakeConnection(
        mockClient,
        "foobar",
        1234,
        diagnosticsSender
    );
    const auto connection = mockClient->AwaitConnection();
    ASSERT_FALSE(connection == nullptr);
    EXPECT_EQ("ws://foobar:1234/", connection->request.target.GenerateString());

    // Assert
    EXPECT_EQ(
        std::future_status::ready,
        results.connectionFuture.wait_for(std::chrono::seconds(1))
    );
    const auto ws = results.connectionFuture.get();
    EXPECT_FALSE(ws == nullptr);
    EXPECT_EQ(
        std::vector< std::string >({
            "MakeConnection[2]: Connecting...",
            "MakeConnection[2]: Connection established.",
        }),
        diagnosticMessages
    );
}

TEST_F(MakeConnectionTests, UpgradeWithoutEngage) {
    // Arrange
    mockClient->behavior = MockClient::Behaviors::UpgradeWithoutEngage;

    // Act
    auto results = WebSockets::MakeConnection(
        mockClient,
        "foobar",
        1234,
        diagnosticsSender
    );

    // Assert
    EXPECT_EQ(
        std::future_status::ready,
        results.connectionFuture.wait_for(std::chrono::seconds(1))
    );
    const auto ws = results.connectionFuture.get();
    EXPECT_TRUE(ws == nullptr);
    EXPECT_EQ(
        std::vector< std::string >({
            "MakeConnection[2]: Connecting...",
            "MakeConnection[5]: Connection upgraded, but failed to engage WebSocket",
        }),
        diagnosticMessages
    );
}

TEST_F(MakeConnectionTests, ConnectionNotUpgraded) {
    // Arrange
    mockClient->behavior = MockClient::Behaviors::ConnectionNotUpgraded;

    // Act
    auto results = WebSockets::MakeConnection(
        mockClient,
        "foobar",
        1234,
        diagnosticsSender
    );

    // Assert
    EXPECT_EQ(
        std::future_status::ready,
        results.connectionFuture.wait_for(std::chrono::seconds(1))
    );
    const auto ws = results.connectionFuture.get();
    EXPECT_TRUE(ws == nullptr);
    EXPECT_EQ(
        std::vector< std::string >({
            "MakeConnection[2]: Connecting...",
            "MakeConnection[5]: Got back response: 404 Not Found",
        }),
        diagnosticMessages
    );
}

TEST_F(MakeConnectionTests, UnableToConnect) {
    // Arrange
    mockClient->behavior = MockClient::Behaviors::UnableToConnect;

    // Act
    auto results = WebSockets::MakeConnection(
        mockClient,
        "foobar",
        1234,
        diagnosticsSender
    );

    // Assert
    EXPECT_EQ(
        std::future_status::ready,
        results.connectionFuture.wait_for(std::chrono::seconds(1))
    );
    const auto ws = results.connectionFuture.get();
    EXPECT_TRUE(ws == nullptr);
    EXPECT_EQ(
        std::vector< std::string >({
            "MakeConnection[2]: Connecting...",
            "MakeConnection[5]: unable to connect",
        }),
        diagnosticMessages
    );
}

TEST_F(MakeConnectionTests, ConnectionBroken) {
    // Arrange
    mockClient->behavior = MockClient::Behaviors::ConnectionBroken;

    // Act
    auto results = WebSockets::MakeConnection(
        mockClient,
        "foobar",
        1234,
        diagnosticsSender
    );

    // Assert
    EXPECT_EQ(
        std::future_status::ready,
        results.connectionFuture.wait_for(std::chrono::seconds(1))
    );
    const auto ws = results.connectionFuture.get();
    EXPECT_TRUE(ws == nullptr);
    EXPECT_EQ(
        std::vector< std::string >({
            "MakeConnection[2]: Connecting...",
            "MakeConnection[5]: connection broken by server",
        }),
        diagnosticMessages
    );
}

TEST_F(MakeConnectionTests, ConnectionTimeOut) {
    // Arrange
    mockClient->behavior = MockClient::Behaviors::ConnectionTimeOut;

    // Act
    auto results = WebSockets::MakeConnection(
        mockClient,
        "foobar",
        1234,
        diagnosticsSender
    );

    // Assert
    EXPECT_EQ(
        std::future_status::ready,
        results.connectionFuture.wait_for(std::chrono::seconds(1))
    );
    const auto ws = results.connectionFuture.get();
    EXPECT_TRUE(ws == nullptr);
    EXPECT_EQ(
        std::vector< std::string >({
            "MakeConnection[2]: Connecting...",
            "MakeConnection[5]: timeout waiting for response",
        }),
        diagnosticMessages
    );
}

TEST_F(MakeConnectionTests, ConnectionAborted) {
    // Arrange
    mockClient->behavior = MockClient::Behaviors::ConnectionAborted;

    // Act
    auto results = WebSockets::MakeConnection(
        mockClient,
        "foobar",
        1234,
        diagnosticsSender
    );
    results.abortConnection();

    // Assert
    EXPECT_EQ(
        std::future_status::ready,
        results.connectionFuture.wait_for(std::chrono::seconds(1))
    );
    const auto ws = results.connectionFuture.get();
    EXPECT_TRUE(ws == nullptr);
    EXPECT_EQ(
        std::vector< std::string >({
            "MakeConnection[2]: Connecting...",
            "MakeConnection[5]: connection aborted",
        }),
        diagnosticMessages
    );
}
