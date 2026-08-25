*** Settings ***
Documentation       Delete a @context whose kind is ImplicitlyCreated without reload param

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource

Test Setup          Create initial ImplicitlyCreated @context
Test Teardown       Delete Initial @context Data


*** Variables ***
${subscription_payload_file_path}=      jsonldContext/subscription-with-implicitlycreated-contexts.jsonld
${reason_204}=                          No Content


*** Test Cases ***
051_06_01 Delete A @context Whose Kind Is ImplicitlyCreated Without Reload Param
    [Documentation]    Check that one can delete a ImplicitlyCreated @context
    [Tags]    ctx-serve    5_13_5    since_v1.5.1

    ${response}=    Delete a @context    ${implicit_id}

    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Reason set to    ${response.reason}    ${reason_204}
    Check Response Does Not Contain Body    ${response}


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

    ${response}=    Retrieve Subscription    ${subscription_id}
    ${data}=    Set Variable    ${response.json()}
    ${implicit_id}=    Get From Dictionary    ${response.json()}    jsonldContext
    ${implicit_id}=    Evaluate    '${implicit_id}'.split('/')[-1]
    Set Global Variable    ${implicit_id}
    Set Suite Variable    ${subscription_id}
    Set Suite Variable    ${list_contexts}
    Check Context Response Body Containing numberOfHits value    ${data}    0

Delete Initial @context Data
    Delete Subscription    ${subscription_id}
    FOR    ${uri}    IN    @{list_contexts}
        Log    URI: ${uri}
        Delete a @context    ${uri}
    END
