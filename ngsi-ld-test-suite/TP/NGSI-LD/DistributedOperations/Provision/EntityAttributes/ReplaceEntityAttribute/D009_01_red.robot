*** Settings ***
Documentation       Verify that, when one has two redirect registrations on a Context Broker, one is able to replace a target entity on both the Context Sources

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup Registration And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_id_prefix}                     urn:ngsi-ld:Vehicle:
${entity_payload_filename}              vehicle-simple-different-attributes.jsonld
${entity_attribute_filename}            fragmentEntities/vehicle-speed-two-datasetid-01-fragment.json
${registration_id_prefix}               urn:ngsi-ld:Registration:
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld


*** Test Cases ***
D009_01_red Replace Entity Attribute
    [Documentation]    Check that one can replace an existing entity attribute, the changes will be forwarded to the Context Sources thanks to the redirect registrations
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-redirect    4_3_6_3    5_6_19    6_3_18

    Set Stub Reply    POST    /broker1/ngsi-ld/v1/entities    201    ${entity_id}
    Set Stub Reply    POST    /broker2/ngsi-ld/v1/entities    201    ${entity_id}

    ${attribute_payload}=    Load Entity    ${entity_attribute_filename}    ${entity_id}
    Set Stub Reply    PUT    /broker1/ngsi-ld/v1/entities/${entity_id}/attrs/speed    204
    Set Stub Reply    PUT    /broker2/ngsi-ld/v1/entities/${entity_id}/attrs/speed    204

    ${response}=    Replace Attribute Selecting Content Type
    ...    entity_id=${entity_id}
    ...    attr_id=speed
    ...    attribute_fragment=${attribute_payload}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}

    ${stub_count}=    Get Stub Count    PUT    /broker1/ngsi-ld/v1/entities/${entity_id}/attrs/speed
    Should Be Equal As Integers    ${stub_count}    1
    ${stub_count}=    Get Stub Count    PUT    /broker2/ngsi-ld/v1/entities/${entity_id}/attrs/speed
    Should Be Equal As Integers    ${stub_count}    1

    # ETSI tool bug fixed: with redirect registrations no local copy exists, and the static CS
    # mocks cannot reflect the replaced value on a federated retrieve (their GET body is a fixed
    # string) - so the original value read-back could never succeed. The forwarding contract
    # (5.6.19.4) is verified by the per-CS stub counts above; additionally verify the forwarded
    # PUT actually carried the replacement value.
    Wait For Request
    ${request_payload}=    Get Request Body
    ${expected_value}=    Convert To String    ${attribute_payload}[speed][value]
    Should Contain    ${request_payload}    ${expected_value}


*** Keywords ***
Setup Registration And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=redirect
    ...    endpoint=/broker1
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id2}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id2}
    ${registration_payload2}=    Prepare Context Source Registration From File
    ...    ${registration_id2}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=redirect
    ...    endpoint=/broker2
    ${response}=    Create Context Source Registration With Return    ${registration_payload2}
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Context Source Registration    ${registration_id2}
