*** Settings ***
Documentation       Check that one can create a implicitlycreated @context through creating a subscription

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Initial @context Data


*** Variables ***
${subscription_payload_file_path}=      jsonldContext/subscription-with-implicitlycreated-contexts.jsonld


*** Test Cases ***
050_03_01 Check The Creation Of ImplicitelyCreted @context
    [Documentation]    Check that one can create a subscription
    [Tags]    ctx-add    5_13_2    since_v1.5.1
    ${subscription_payload}=    Load JSON From File    ${EXECDIR}/data/${subscription_payload_file_path}

    ${subscription_id}=    Get Value From JSON    ${subscription_payload}    $..id
    ${subscription_id}=    Get From List    ${subscription_id}    0

    ${list_contexts}=    Get Value From JSON    ${subscription_payload}    $..@context
    ${list_contexts}=    Get From List    ${list_contexts}    0

    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}

    Set Suite Variable    ${subscription_id}
    Set Suite Variable    ${list_contexts}

    ${response}=    List @contexts    true    ImplicitlyCreated

    Check Response Status Code    200    ${response.status_code}

    ${data}=    Get From List    ${response.json()}    0
    ${implicit_id}=    Get From Dictionary    ${data}    URL
    Set Global Variable    ${implicit_id}

    Check Response Kind set to    ${data}    ImplicitlyCreated


*** Keywords ***
Delete Initial @context Data
    Delete Subscription    ${subscription_id}
    FOR    ${uri}    IN    @{list_contexts}
        Log    URI: ${uri}
        Delete a @context    ${uri}
    END
    Delete a @context    ${implicit_id}
