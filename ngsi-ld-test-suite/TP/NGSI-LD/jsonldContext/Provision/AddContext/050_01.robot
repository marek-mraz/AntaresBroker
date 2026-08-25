*** Settings ***
Documentation       Check that one can add a hosted @context

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Variables           ${EXECDIR}/resources/variables.py

Test Teardown       Delete Initial @context
Test Template       Add a valid @context


*** Variables ***
${filename_dictionary}=     @context-minimal-valid.json
${filename_list}=           @context-cached-valid.json
${reason_201}=              Created
${reason_204}=              No Content


*** Test Cases ***    FILENAME    CONTEXT_TYPE
050_01_01 Add A Valid Hosted @context From Key=value
    [Tags]    ctx-add    5_13_2    since_v1.5.1
    ${filename_dictionary}    Hosted
050_01_02 Add A Valid Cached @context From URI
    [Tags]    ctx-add    5_13_2    since_v1.5.1
    ${filename_list}    Hosted


*** Keywords ***
Add a valid @context
    [Documentation]    Check that one can add a @context
    [Arguments]    ${filename}    ${context_type}

    ${response}=    Add a new @context    ${filename}

    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Does Not Contain Body    ${response}
    Check Response Reason set to    ${response.reason}    ${reason_201}

    Dictionary Should Contain Key    ${response.headers}    Location    msg=HTTP Headers do not contain key 'Location'
    ${uri}=    Fetch Id From Response Location Header    ${response.headers}
    Set Suite Variable    ${uri}

    Log    URI: ${uri}

    # Need to check that the kind value of the created context is "hosted"
    ${response_serve}=    Serve a @context    ${uri}    true
    Check Response Kind set to    ${response_serve.json()}    ${context_type}

    Log    URI: ${response_serve}

Delete Initial @context
    ${response}=    List @contexts    true    ${EMPTY}
    # One needs to extract all the contexts except the core context and delete them
    FOR    ${item}    IN    @{response.json()}
        ${uri}=    Get From Dictionary    ${item}    URL
        IF    '${uri}'=='${core_context}'
            Log    WARNING, Trying to delete the Core Context
        ELSE
            Delete a @context    ${uri}
        END
    END
