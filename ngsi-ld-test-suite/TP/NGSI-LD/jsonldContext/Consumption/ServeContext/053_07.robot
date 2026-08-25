*** Settings ***
Documentation       Check that one can serve a ImplicitlyCreated @context with details set to true

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create initial ImplicitlyCreated @context
Test Teardown       Delete Initial @context Data


*** Variables ***
${subscription_payload_file_path}=      jsonldContext/subscription-with-implicitlycreated-contexts.jsonld
${reason_200}=                          OK


*** Test Cases ***
053_07_01 Check That One Can Serve A ImplicitlyCreated @context With Details Set To True
    [Documentation]    Check that one can serve a ImplicitlyCreated @context with details set to true
    [Tags]    ctx-serve    5_13_4    since_v1.5.1

    ${response}=    Serve a @context
    ...    contextId=${implicit_id}
    ...    details=true

    Check Response Status Code    200    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_200}
    Check Response Headers Containing Content-Type set to    ${CONTENT_TYPE_JSON}    ${response.headers}

    # Check mandatory keys in the response (URL, localId, kind, timestamp) and their possible values
    Check Context Response Body Containing Detailed Information    ${response.json()}    ImplicitlyCreated

    # Check optional keys in the response (lastUsage, numberOfHits, extraInfo) and their possible values
    Check Response Body Might Contain Optional Fields    ${response.json()}    lastUsage
    Check Response Body Might Contain Optional Fields    ${response.json()}    numberOfHits
    Check Response Body Might Contain Optional Fields    ${response.json()}    extraInfo

    # Check that there is no other keys
    Check Context Detailed Information Keys    ${response.json()}

    # One needs to check the list of responses
    # Check Context Response Body Containing a JSONObject with details of a ImplicitlyCreated @contexts
    # ...    response=${response.json()}
    # ...    expected_length=1


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

    ${response}=    Retrieve Subscription    ${subscription_id}
    ${data}=    Set Variable    ${response.json()}
    ${implicit_id}=    Get From Dictionary    ${response.json()}    jsonldContext
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
