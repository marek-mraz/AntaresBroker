*** Settings ***
Documentation       Check that one can add a hosted @context with list of URIs and each of them are cached @coxtexts

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Variables           ${EXECDIR}/resources/variables.py

Test Teardown       Delete Initial @context


*** Variables ***
${filename_list}=       @context-cached-valid.json
${entity_filename}=     building-simple-attributes.json
${reason_201}=          Created
${reason_204}=          No Content
${content_type}=        application/json


*** Test Cases ***
050_04_01 Add A Valid Hosted @context With URIs And Check That The URIs Are Cached @contexts
    [Documentation]    Check that one can add a @context
    [Tags]    ctx-add    5_13_2    since_v1.5.1

    ${response}=    Add a new @context    ${filename_list}

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
    Check Response Kind set to    ${response_serve.json()}    Hosted
    ${entity_id}=    Generate Random Building Entity Id
    # ${url} already contains the /ngsi-ld/v1 prefix and ${uri} is the full Location path
    # (/ngsi-ld/v1/jsonldContexts/...). Concatenating both doubled the prefix, yielding an
    # unresolvable .../ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/... URL (405 -> LdContextNotAvailable).
    # The hosted @context absolute URL is the broker host + the Location path.
    Create Entity Selecting Content Type
    ...    ${entity_filename}
    ...    ${entity_id}
    ...    ${content_type}
    ...    http://localhost:9090${uri}

    Delete Entity    ${entity_id}
    # Need to check that each of the URIs are Cached @contexts
    Check Cached @Contexts    ${filename_list}


*** Keywords ***
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
