*** Settings ***
Documentation       Check that one cannot delete a ImplicitlyCreated @context with reload set to true

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Create initial ImplicitlyCreated @context
Test Teardown       Delete Initial @context Data


*** Variables ***
${subscription_payload_file_path}=      jsonldContext/subscription-with-implicitlycreated-contexts.jsonld
${reason_400}=                          Bad Request


*** Test Cases ***
051_07_01 Delete A ImplicitlyCreated @contexts With A Valid Id And Reload Set To True
    [Documentation]    Check that one cannot delete a ImplicitlyCreated @context with reload set to true
    [Tags]    ctx-delete    5_13_5    since_v1.5.1

    ${response}=    Delete a @context    ${implicit_id}    true

    Check Response Status Code    400    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_400}
    Check Response Body Containing ProblemDetails Element    ${response.json()}    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Create initial ImplicitlyCreated @context
    ${subscription_payload}=    Load JSON From File    ${EXECDIR}/data/${subscription_payload_file_path}

    ${subscription_id}=    Get Value From JSON    ${subscription_payload}    $..id
    ${subscription_id}=    Get From List    ${subscription_id}    0

    ${list_contexts}=    Get Value From JSON    ${subscription_payload}    $..@context
    ${list_contexts}=    Get From List    ${list_contexts}    0

    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}

    ${response}=    List @contexts    true    ImplicitlyCreated

    Check Response Status Code    200    ${response.status_code}

    ${data}=    Get From List    ${response.json()}    0
    ${implicit_id}=    Get From Dictionary    ${data}    URL
    # Take only the localId (last path segment). The URL field is a full
    # broker URL (http://host/ngsi-ld/v1/jsonldContexts/<id>) and TS 104-175
    # § 13.5.3 says the DELETE path takes the locally unique id, not the URL.
    ${implicit_id}=    Evaluate    '${implicit_id}'.split('/')[-1]

    Set Global Variable    ${implicit_id}
    Set Suite Variable    ${subscription_id}
    Set Suite Variable    ${list_contexts}

Delete Initial @context Data
    Delete Subscription    ${subscription_id}
    FOR    ${uri}    IN    @{list_contexts}
        Log    URI: ${uri}
        Delete a @context    ${uri}
    END
    Delete a @context    ${implicit_id}
