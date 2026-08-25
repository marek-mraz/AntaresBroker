*** Settings ***
Documentation       Check that one can delete a previous created cached @context without reload param

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Variables           ${EXECDIR}/resources/variables.py

Test Setup          Create Initial cached @context
Test Teardown       Delete Initial @context


*** Variables ***
${filename}=            @context-cached-one-valid.json
${entity_filename}=     building-simple-attributes.json
${content_type}=        application/json
${reason_204}=          No Content


*** Test Cases ***
051_03_01 Delete A @context Whose Kind Is Cached Without Reload Param
    [Documentation]    Check that one can delete a cached @context
    [Tags]    ctx-serve    5_13_5    since_v1.5.1

    ${response}=    Delete a @context    ${contextId}

    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Reason set to    ${response.reason}    ${reason_204}
    Check Response Does Not Contain Body    ${response}


*** Keywords ***
Create Initial cached @context
    ${entity_payload}=    Load JSON From File    ${EXECDIR}/data/jsonldContext/${filename}
    ${contextId}=    Get From Dictionary    ${entity_payload}    @context
    ${contextId}=    Get From List    ${contextId}    0

    ${response}=    Add a new @context    ${filename}
    Check Response Status Code    201    ${response.status_code}
    ${uri}=    Fetch Id From Response Location Header    ${response.headers}
    ${entity_id}=    Generate Random Building Entity Id
    # ${url} already contains the /ngsi-ld/v1 prefix and ${uri} is the full Location path, so
    # ${url}${uri} doubled the prefix (.../ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/... -> 405 ->
    # LdContextNotAvailable). Use the broker host + the Location path instead.
    ${response}=    Create Entity Selecting Content Type
    ...    ${entity_filename}
    ...    ${entity_id}
    ...    ${content_type}
    ...    http://localhost:9090${uri}

    ${response}=    Delete Entity    ${entity_id}
    ${response}=    Serve a @context    ${contextId}    true
    Check Response Kind set to    ${response.json()}    Cached

    Set Global Variable    ${contextId}

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
