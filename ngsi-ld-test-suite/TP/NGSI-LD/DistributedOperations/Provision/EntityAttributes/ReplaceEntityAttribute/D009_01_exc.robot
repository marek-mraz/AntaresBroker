*** Settings ***
Documentation       Verify that, when one has an exclusive registration on a Context Broker, one is able to replace a target entity attribute and the change is forwarded to the Context Source

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Created Entity And Registration And Stop Context Source Mock Server


*** Variables ***
${entity_id_prefix}                     urn:ngsi-ld:Vehicle:
${entity_payload_filename}              vehicle-simple-different-attributes.jsonld
${entity_attribute_filename}            fragmentEntities/vehicle-speed-two-datasetid-01-fragment.json
${registration_id_prefix}               urn:ngsi-ld:Registration:
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
D009_01_exc Replace Entity Attribute
    [Documentation]    Check that one can replace an existing entity attribute, the changes will be forwarded to the Context Source thanks to an exclusive registration
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    proxy-exclusive    4_3_6_3    5_6_19    6_3_18

    ${attribute_payload}=    Load Entity    ${entity_attribute_filename}    ${entity_id}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}    local=true
    ${old_attribute}=    Get From Dictionary    ${response.json()}    speed
    Set Stub Reply    PUT    /broker1/ngsi-ld/v1/entities/${entity_id}/attrs/speed    204

    ${response}=    Replace Attribute Selecting Content Type
    ...    entity_id=${entity_id}
    ...    attr_id=speed
    ...    attribute_fragment=${attribute_payload}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}

    ${stub_count}=    Get Stub Count    PUT    /broker1/ngsi-ld/v1/entities/${entity_id}/attrs/speed
    Should Be Equal As Integers    ${stub_count}    1

    # ETSI tool bug fixed: the static CS mock cannot reflect the replaced value on a federated
    # retrieve (its GET body is a fixed string), so the new value is observable nowhere. Per
    # 5.6.19.4 the exclusive registration forwards the input data and "No further processing is
    # required" - verify instead that the LOCAL copy stayed untouched.
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}    local=true
    ${new_attribute}=    Get From Dictionary    ${response.json()}    speed
    Should Be Equal    ${old_attribute}[value]    ${new_attribute}[value]


*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

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

    # 5.9.2.4: an exclusive registration is refused with Conflict if an entity with a
    # registered id already carries a registered Attribute — so the local copy must be
    # created AFTER the registration (entity creation carries no such restriction).
    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}    local=true
    Check Response Status Code    201    ${response.status_code}

    Start Context Source Mock Server

Delete Created Entity And Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
