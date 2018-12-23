/**
 * @file MakeConnection.cpp
 *
 * This module contains the implementation of the WebSockets::MakeConnection
 * function.
 *
 * © 2018 by Richard Walters
 */

#include <future>
#include <Http/IClient.hpp>
#include <stdint.h>
#include <string>
#include <SystemAbstractions/DiagnosticsSender.hpp>
#include <WebSockets/MakeConnection.hpp>
#include <WebSockets/WebSocket.hpp>

namespace {

    /**
     * This is the period between polling the abort promise when awaiting the
     * completion of the HTTP client transaction when attempting to make a new
     * connection.
     */
    constexpr std::chrono::milliseconds ABORT_POLLING_PERIOD = std::chrono::milliseconds(50);

    /**
     * This method is called to synchronously attempt to connect to a web
     * server and upgrade the connection a WebSocket.
     *
     * @param[in] http
     *     This is the web client object to use to make the connection.
     *
     * @param[in] host
     *     This is the host name or IP address of the server to which to
     *     connect.
     *
     * @param[in] port
     *     This is the port number of the server to which to connect.
     *
     * @param[in] diagnosticsSender
     *     This is the function to call to publish any diagnostic messages.
     *
     * @param[in] aborted
     *     This is a promise that is set if the connection attempt is aborted.
     *
     * @return
     *     The new WebSocket connection to the server is returned.
     *
     * @retval nullptr
     *     This is returned if the connection could not be made.
     */
    std::shared_ptr< WebSockets::WebSocket > MakeConnectionSynchronous(
        std::shared_ptr< Http::IClient > http,
        std::string host,
        uint16_t port,
        std::shared_ptr< SystemAbstractions::DiagnosticsSender > diagnosticsSender,
        std::shared_ptr< std::promise< void > > aborted
    ) {
        diagnosticsSender->SendDiagnosticInformationString(
            2,
            "Connecting..."
        );

        // Set up a client-side WebSocket and form the HTTP request for it.
        const auto ws = std::make_shared< WebSockets::WebSocket >();
        Http::Request request;
        request.method = "GET";
        request.target.SetScheme("ws");
        request.target.SetHost(host);
        request.target.SetPort(port);
        request.target.SetPath({""});
        ws->StartOpenAsClient(request);

        // Use the HTTP client to send the request, providing a callback if the
        // connection was successfully upgraded to the WebSocket protocol.
        bool wsEngaged = false;
        const auto transaction = http->Request(
            request,
            true,
            [
                ws,
                &wsEngaged
            ](
                const Http::Response& response,
                std::shared_ptr< Http::Connection > connection,
                const std::string& trailer
            ){
                if (ws->FinishOpenAsClient(connection, response)) {
                    wsEngaged = true;
                }
            }
        );
        // TODO: Remove polling once we can register a delegate with the
        // transaction that is called upon completion.
        auto wasAborted = aborted->get_future();
        while (!transaction->AwaitCompletion(ABORT_POLLING_PERIOD)) {
            if (wasAborted.wait_for(std::chrono::seconds(0)) == std::future_status::ready) {
                diagnosticsSender->SendDiagnosticInformationString(
                    SystemAbstractions::DiagnosticsSender::Levels::WARNING,
                    "connection aborted"
                );
                return nullptr;
            }
        }
        switch (transaction->state) {
            case Http::IClient::Transaction::State::Completed: {
                if (wsEngaged) {
                    diagnosticsSender->SendDiagnosticInformationString(
                        2,
                        "Connection established."
                    );
                } else {
                    if (transaction->response.statusCode == 101) {
                        diagnosticsSender->SendDiagnosticInformationString(
                            SystemAbstractions::DiagnosticsSender::Levels::WARNING,
                            "Connection upgraded, but failed to engage WebSocket"
                        );
                    } else {
                        diagnosticsSender->SendDiagnosticInformationFormatted(
                            SystemAbstractions::DiagnosticsSender::Levels::WARNING,
                            "Got back response: %u %s",
                            transaction->response.statusCode,
                            transaction->response.reasonPhrase.c_str()
                        );
                    }
                }
            } break;

            case Http::IClient::Transaction::State::UnableToConnect: {
                diagnosticsSender->SendDiagnosticInformationString(
                    SystemAbstractions::DiagnosticsSender::Levels::WARNING,
                    "unable to connect"
                );
            } break;

            case Http::IClient::Transaction::State::Broken: {
                diagnosticsSender->SendDiagnosticInformationString(
                    SystemAbstractions::DiagnosticsSender::Levels::WARNING,
                    "connection broken by server"
                );
            } break;

            case Http::IClient::Transaction::State::Timeout: {
                diagnosticsSender->SendDiagnosticInformationString(
                    SystemAbstractions::DiagnosticsSender::Levels::WARNING,
                    "timeout waiting for response"
                );
            } break;

            default: {
                diagnosticsSender->SendDiagnosticInformationFormatted(
                    SystemAbstractions::DiagnosticsSender::Levels::ERROR,
                    "Unknown transaction state (%d)",
                    (int)transaction->state
                );
            } break;
        }
        return wsEngaged ? ws : nullptr;
    }

}

namespace WebSockets {

    MakeConnectionResults MakeConnection(
        std::shared_ptr< Http::IClient > http,
        const std::string& host,
        uint16_t port,
        std::shared_ptr< SystemAbstractions::DiagnosticsSender > diagnosticsSender
    ) {
        MakeConnectionResults results;
        const auto aborted = std::make_shared< std::promise< void > >();
        results.connectionFuture = std::async(
            std::launch::async,
            MakeConnectionSynchronous,
            http,
            host,
            port,
            diagnosticsSender,
            aborted
        );
        results.abortConnection = [aborted]{
            aborted->set_value();
        };
        return results;
    }

}
