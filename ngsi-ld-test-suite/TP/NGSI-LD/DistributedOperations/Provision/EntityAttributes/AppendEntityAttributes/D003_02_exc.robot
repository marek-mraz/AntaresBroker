*** Settings ***
Documentation       Check that if one request the Context Broker to append an attribute whose id matches an exclusive registration with noOverwrite flag, the entity is updated on the Context Broker and the update is forwarded to the Context Source but existing attributes are not overwritten

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${fragment_filename}                    vehicle-speed-isParked-fragment.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld

*** Test Cases ***
D003_02_exc Append Entity Attribute
    [Documentation]    Check that an entity attribute is appended and the exclusive registration forwards the request to the Context Source with the noOverwrite flag
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_6_3

    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entities/${entity_id}/attrs/    207
    # ETSI tool bug fixed: without a JSON-LD context the fragment expands against the default
    # @context (6.3.5) and can never match the registration's compound-context propertyNames.
    ${response}=    Append Entity Attributes With Parameters
    ...    ${entity_id}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_JSON}
    ...    noOverwrite
    ...    context=${ngsild_test_suite_context}
    Wait for redirected request
    Check Response Status Code    207   ${response.status_code}

    ${stub}=    Get Request Url Params    options
    Should Contain    ${stub}    noOverwrite

*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}    local=true
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=exclusive
    ...    endpoint=/broker1
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
